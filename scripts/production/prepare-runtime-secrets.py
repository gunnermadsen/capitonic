#!/usr/bin/env python3
"""Select immutable AWS versions and consumer-specific rollout tokens without exposing values."""
import argparse
import base64
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import uuid
import re
import urllib.request
import urllib.error

ROOT = Path(__file__).resolve().parents[2]
COMPONENTS = ('ingester', 'polymarket-bot')


def command(*args):
    return subprocess.check_output(args, text=True)


def load_values(component, revision=None):
    if revision:
        source = command("git", "-C", str(ROOT), "show", f"{revision}:capitonic-helm-chart/environments/production/{component}.yaml")
        return json.loads(subprocess.check_output(["yq", "-o=json", ".", "-"], text=True, input=source))
    return json.loads(command('yq', '-o=json', '.', str(ROOT / f'capitonic-helm-chart/environments/production/{component}.yaml')))


def prepare_headlamp(args):
    """Admit only Headlamp's credentials; leave application admission unchanged."""
    values = load_values('headlamp', args.revision)
    config = values['externalSecrets']
    entry, = config['entries']
    expected = {'HEADLAMP_USERNAME': 'HEADLAMP_USERNAME', 'HEADLAMP_PASSWORD': 'HEADLAMP_PASSWORD'}
    if not config['enabled'] or entry['name'] != 'headlamp-basic-auth' or entry['properties'] != expected:
        raise RuntimeError('Unexpected Headlamp credential mapping')
    request = ['aws', 'secretsmanager', 'get-secret-value', '--region', config['region'],
               '--secret-id', config['secretId'], '--output', 'json']
    if args.verify:
        request += ['--version-id', entry['version']]
    response = json.loads(command(*request))
    source = json.loads(response['SecretString'])
    username, password = source.get('HEADLAMP_USERNAME'), source.get('HEADLAMP_PASSWORD')
    if not isinstance(username, str) or not re.fullmatch(r'[A-Za-z0-9._-]+', username):
        raise RuntimeError('Missing or invalid HEADLAMP_USERNAME')
    if not isinstance(password, str) or not 16 <= len(password.encode()) <= 72 or '\n' in password or '\r' in password:
        raise RuntimeError('Missing or invalid HEADLAMP_PASSWORD')
    if not args.verify:
        changed = not entry.get('version')
        if entry.get('version') and entry['version'] != response['VersionId']:
            previous = json.loads(command(*request, '--version-id', entry['version']))
            old_source = json.loads(previous['SecretString'])
            changed = any(old_source.get(key) != source[key] for key in expected)
        if changed:
            entry['version'] = response['VersionId']
            entry['refreshAfter'] = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
        print(json.dumps({'headlamp': {'externalSecrets': config}}, sort_keys=True))
        return
    external = json.loads(command('kubectl', '-n', 'capitonic', 'get', 'externalsecret', entry['name'], '-o', 'json'))
    actual_refs = {item['secretKey']: (item['remoteRef']['key'], item['remoteRef']['property'], item['remoteRef']['version'])
                   for item in external['spec']['data']}
    wanted_refs = {key: (config['secretId'], prop, 'uuid/' + entry['version']) for key, prop in expected.items()}
    if actual_refs != wanted_refs or external['spec']['refreshPolicy'] != 'OnChange':
        raise RuntimeError('Headlamp ExternalSecret does not match admitted properties/version')
    status = external.get('status', {})
    if not status.get('syncedResourceVersion', '').startswith(str(external['metadata']['generation']) + '-') or not any(
            c['type'] == 'Ready' and c['status'] == 'True' for c in status.get('conditions', [])):
        raise RuntimeError('Headlamp ExternalSecret has not reconciled its current generation')
    secret = json.loads(command('kubectl', '-n', 'capitonic', 'get', 'secret', entry['name'], '-o', 'json'))
    if set(secret['data']) != {'users'} or not base64.b64decode(secret['data']['users']).decode().startswith(username + ':$2'):
        raise RuntimeError('Headlamp target Secret is not the expected bcrypt users entry')
    root = 'https://' + values['upstream']['ingress']['hosts'][0]['host'] + '/system'

    def get(path, credential=None):
        headers = {}
        if credential:
            headers['Authorization'] = 'Basic ' + base64.b64encode(credential.encode()).decode()
        try:
            with urllib.request.urlopen(urllib.request.Request(root + path, headers=headers), timeout=15) as result:
                return result.status, result.read()
        except urllib.error.HTTPError as error:
            return error.code, b''

    credential = username + ':' + password
    for description, supplied, wanted in [('missing', None, 401), ('wrong', username + ':invalid-password', 401),
                                          ('valid', credential, 200)]:
        code, _ = get('/', supplied)
        if code != wanted:
            raise RuntimeError(f'Headlamp {description} credential check returned HTTP {code}')
    context = values['upstream']['config']['inClusterContextName']
    metrics_path = f'/clusters/{context}/apis/metrics.k8s.io/v1beta1/'
    for resource in ('nodes', 'namespaces/capitonic/pods'):
        code, body = get(metrics_path + resource, credential)
        metrics = json.loads(body) if code == 200 else {}
        if not metrics.get('items'):
            raise RuntimeError(f'Headlamp {resource} metrics unavailable: HTTP {code}')
        for item in metrics['items']:
            timestamp = datetime.fromisoformat(item['timestamp'].replace('Z', '+00:00'))
            if not 0 <= (datetime.now(timezone.utc) - timestamp).total_seconds() <= 120:
                raise RuntimeError('Headlamp resource metrics are stale')
        print(f'Headlamp fresh {resource} metrics verified ({len(metrics["items"])} items)')
    code, _ = get(f'/clusters/{context}/api/v1/namespaces/capitonic/secrets', credential)
    if code != 403:
        raise RuntimeError(f'Headlamp viewer secret access should be denied, got HTTP {code}')
    print('Headlamp admitted AWS version reconciled by ESO; password login and read-only metrics verified')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verify', action='store_true')
    parser.add_argument('--component', choices=(*COMPONENTS, 'headlamp'))
    parser.add_argument('--revision', help='Immutable rollback revision to verify')
    args = parser.parse_args()
    components = (args.component,) if args.component else COMPONENTS
    if args.revision and not args.verify:
        parser.error("--revision requires --verify")
    if args.component == 'headlamp':
        prepare_headlamp(args)
        return
    values = {component: (load_values(component, args.revision) if args.revision else load_values(component)) for component in components}
    credentials = {component: dict(v.get('credentialRevisions', {})) for component, v in values.items()}
    if not args.verify:
        credentials["grafana"] = load_values("grafana").get("credentialRevisions", {})
    aws_cache = {}
    changed_consumers = set()
    now = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    for component, configuration in values.items():
        config = configuration['externalSecrets']
        for entry in config['entries']:
            version = entry['version'] if args.verify else None
            cache_key = (config['secretId'], version)
            if cache_key not in aws_cache:
                request = ['aws', 'secretsmanager', 'get-secret-value', '--region', config['region'], '--secret-id', config['secretId'], '--output', 'json']
                if version:
                    request += ['--version-id', version]
                response = json.loads(command(*request))
                aws_cache[cache_key] = (response['VersionId'], json.loads(response['SecretString']))
            selected_version, source = aws_cache[cache_key]
            projected = {}
            for key, property_name in entry['properties'].items():
                value = source.get(property_name)
                if not isinstance(value, str):
                    raise RuntimeError(f"Missing string AWS property {property_name} for {entry['name']}; no secret changed")
                projected[key] = base64.b64encode(value.encode()).decode()
            actual = json.loads(command('kubectl', '-n', 'capitonic', 'get', 'secret', entry['name'], '-o', 'json'))['data']
            if set(actual) != set(projected):
                raise RuntimeError(f"Key set mismatch for {entry['name']}; refusing to discard existing keys")
            differs = any(actual.get(key) != value for key, value in projected.items())
            mapping_requires_version = False
            if not args.verify and entry.get('version') and entry['version'] != selected_version:
                old_key = (config['secretId'], entry['version'])
                if old_key not in aws_cache:
                    old_response = json.loads(command('aws', 'secretsmanager', 'get-secret-value', '--region', config['region'], '--secret-id', config['secretId'], '--version-id', entry['version'], '--output', 'json'))
                    aws_cache[old_key] = (old_response['VersionId'], json.loads(old_response['SecretString']))
                old_source = aws_cache[old_key][1]
                mapping_requires_version = any(old_source.get(prop) != source[prop] for prop in entry['properties'].values())
            if args.verify:
                if differs:
                    raise RuntimeError(f"Selected AWS version does not match {entry['name']}")
                external = json.loads(command('kubectl', '-n', 'capitonic', 'get', 'externalsecret', entry['name'], '-o', 'json'))
                if not all(item['remoteRef']['version'] == 'uuid/' + selected_version for item in external['spec']['data']):
                    raise RuntimeError(f"ExternalSecret version mismatch for {entry['name']}")
                expected_generation = str(external['metadata']['generation']) + '-'
                if not external.get('status', {}).get('syncedResourceVersion', '').startswith(expected_generation):
                    raise RuntimeError(f"ExternalSecret generation not synchronized: {entry['name']}")
                if not any(c['type'] == 'Ready' and c['status'] == 'True' for c in external.get('status', {}).get('conditions', [])):
                    raise RuntimeError(f"ExternalSecret not Ready: {entry['name']}")
            elif differs or mapping_requires_version or not entry.get('version'):
                entry['version'] = selected_version
                entry['refreshAfter'] = now
                if differs:
                    changed_consumers.update(entry['consumers'])
    if args.verify:
        print('Selected AWS JSON properties exactly match all managed Kubernetes Secrets')
        return
    for consumer in changed_consumers:
        owner = consumer if consumer in ('polymarket-bot', 'grafana') else 'ingester'
        credentials[owner][consumer] = str(uuid.uuid4())
    print(json.dumps({component: {'externalSecrets': values[component]['externalSecrets'], 'credentialRevisions': credentials[component]} for component in components} | ({'grafana': {'credentialRevisions': credentials['grafana']}} if not args.verify else {}), sort_keys=True))


if __name__ == '__main__':
    main()
