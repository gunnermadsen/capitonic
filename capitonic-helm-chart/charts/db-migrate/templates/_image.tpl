{{- define "db-migrate.image" -}}
{{- if eq .Values.environment "production" -}}
{{- regexReplaceAll "-rc\\.[0-9]+$" .Values.image "" -}}
{{- else -}}
{{- .Values.image -}}
{{- end -}}
{{- end -}}
