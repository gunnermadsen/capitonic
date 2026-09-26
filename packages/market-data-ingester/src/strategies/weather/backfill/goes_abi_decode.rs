use std::{collections::BTreeMap, path::Path};

use netcdf_reader::NcFile;
use netcdf_writer::{NcAttrValue, NcFileBuilder, NcWriteFormat, NcWriteOptions};

const STATION_LATITUDE: f64 = 40.7769;
const STATION_LONGITUDE: f64 = -73.8740;
const RADII_KM: [u32; 3] = [25, 50, 100];
const SECTORS: [&str; 5] = ["all", "north", "south", "east", "west"];

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub mean: Option<f64>,
    pub stddev: Option<f64>,
    pub p10: Option<f64>,
    pub p50: Option<f64>,
    pub p90: Option<f64>,
    pub max: Option<f64>,
    pub valid_pixel_fraction: f64,
    pub clear_fraction: Option<f64>,
    pub cloudy_fraction: Option<f64>,
    pub quality_counts: BTreeMap<i64, usize>,
    pub quality_valid: usize,
}

#[derive(Clone, Debug)]
pub struct Patch {
    pub variable: String,
    pub units: Option<String>,
    pub width: usize,
    pub height: usize,
    pub values: Vec<f64>,
    pub latitude: Vec<f64>,
    pub longitude: Vec<f64>,
    pub dqf: Option<Vec<f64>>,
    pub summaries: BTreeMap<(u32, &'static str), Summary>,
}

impl Patch {
    pub fn matches_netcdf(
        &self,
        path: &Path,
        product_key: &str,
        source_product: &str,
    ) -> Result<bool, String> {
        let file = NcFile::open(path).map_err(|error| error.to_string())?;
        let expected = [
            (product_key, self.values.as_slice(), 2e-5),
            ("latitude", self.latitude.as_slice(), 5e-4),
            ("longitude", self.longitude.as_slice(), 4e-5),
        ];
        for (name, values, tolerance) in expected {
            let actual = file
                .read_variable_as_f64(name)
                .map_err(|error| error.to_string())?;
            if actual.shape() != [self.height, self.width]
                || actual
                    .iter()
                    .zip(values)
                    .any(|(a, b)| !equal_float(*a, *b, tolerance))
            {
                return Ok(false);
            }
        }
        match &self.dqf {
            Some(values) => {
                let actual = file
                    .read_variable_as_f64("DQF")
                    .map_err(|error| error.to_string())?;
                if actual.shape() != [self.height, self.width]
                    || actual
                        .iter()
                        .zip(values)
                        .any(|(a, b)| !equal_float(*a, *b, 0.0))
                {
                    return Ok(false);
                }
            }
            None if file.variable("DQF").is_ok() => return Ok(false),
            None => {}
        }
        let attrs = file
            .global_attributes()
            .map_err(|error| error.to_string())?;
        for (name, value) in [
            ("source_product", source_product),
            ("feature_schema_version", "goes-abi-klga-v2"),
            ("station_id", "KLGA"),
        ] {
            if !attrs
                .iter()
                .any(|attr| attr.name == name && attr.value.as_string().as_deref() == Some(value))
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn netcdf_bytes(&self, product_key: &str, source_product: &str) -> Result<Vec<u8>, String> {
        let mut file = NcFileBuilder::new();
        let y = file
            .add_dimension("y", self.height as u64)
            .map_err(|error| error.to_string())?;
        let x = file
            .add_dimension("x", self.width as u64)
            .map_err(|error| error.to_string())?;
        file.add_attribute("source_product", NcAttrValue::Chars(source_product.into()))
            .map_err(|error| error.to_string())?;
        file.add_attribute(
            "feature_schema_version",
            NcAttrValue::Chars("goes-abi-klga-v2".into()),
        )
        .map_err(|error| error.to_string())?;
        file.add_attribute("station_id", NcAttrValue::Chars("KLGA".into()))
            .map_err(|error| error.to_string())?;
        for (name, values) in [
            (product_key, self.values.as_slice()),
            ("latitude", self.latitude.as_slice()),
            ("longitude", self.longitude.as_slice()),
        ] {
            let variable = file
                .add_variable::<f64>(name, &[y, x])
                .map_err(|error| error.to_string())?;
            file.set_variable_deflate(variable, Some(4), false)
                .map_err(|error| error.to_string())?;
            file.write_variable(variable, values)
                .map_err(|error| error.to_string())?;
        }
        if let Some(flags) = &self.dqf {
            let variable = file
                .add_variable::<f64>("DQF", &[y, x])
                .map_err(|error| error.to_string())?;
            file.set_variable_deflate(variable, Some(4), false)
                .map_err(|error| error.to_string())?;
            file.write_variable(variable, flags)
                .map_err(|error| error.to_string())?;
        }
        let (_, bytes) = file
            .to_vec(NcWriteOptions {
                format: NcWriteFormat::Nc4,
            })
            .map_err(|error| error.to_string())?;
        Ok(bytes)
    }
}

fn equal_float(first: f64, second: f64, tolerance: f64) -> bool {
    (first.is_nan() && second.is_nan()) || (first - second).abs() <= tolerance
}

pub fn decode(path: &Path, candidates: &[&str], clear_sky: bool) -> Result<Patch, String> {
    let file = NcFile::open(path).map_err(|error| error.to_string())?;
    let variable = candidates
        .iter()
        .find(|name| file.variable(name).is_ok())
        .ok_or_else(|| format!("none of the expected GOES variables exists: {candidates:?}"))?;
    let shape = file
        .variable(variable)
        .map_err(|error| error.to_string())?
        .shape();
    if shape.len() != 2 {
        return Err(format!("GOES variable {variable} is not two-dimensional"));
    }
    let height = usize::try_from(shape[0]).map_err(|error| error.to_string())?;
    let width = usize::try_from(shape[1]).map_err(|error| error.to_string())?;
    let values = read_xarray_f32(&file, variable)?;
    let dqf = if file.variable("DQF").is_ok() {
        Some(read_xarray_f32(&file, "DQF")?)
    } else {
        None
    };
    let x = read_xarray_f32(&file, "x")?;
    let y = read_xarray_f32(&file, "y")?;
    if values.len() != width * height
        || x.len() != width
        || y.len() != height
        || dqf
            .as_ref()
            .is_some_and(|flags| flags.len() != values.len())
    {
        return Err("GOES fixed-grid dimensions did not match".into());
    }
    let projection = file
        .variable("goes_imager_projection")
        .map_err(|_| "GOES projection metadata is missing".to_owned())?;
    let height_m = attribute(projection, "perspective_point_height")?;
    let origin = attribute(projection, "longitude_of_projection_origin")?.to_radians();
    let major = attribute(projection, "semi_major_axis")?;
    let minor = attribute(projection, "semi_minor_axis")?;
    let satellite_height = height_m + major;
    let axis_ratio = major * major / (minor * minor);
    let units = file
        .variable(variable)
        .ok()
        .and_then(|v| v.attribute("units"))
        .and_then(|v| v.value.as_string());

    let mut latitude = Vec::with_capacity(values.len());
    let mut longitude = Vec::with_capacity(values.len());
    let mut bounds = (usize::MAX, usize::MAX, 0, 0);
    for (row, &yy) in y.iter().enumerate() {
        for (column, &xx) in x.iter().enumerate() {
            let (lat, lon) = project(xx, yy, satellite_height, axis_ratio, major, origin);
            latitude.push(lat);
            longitude.push(lon);
            if in_sector(lat, lon, 100, "all") {
                bounds.0 = bounds.0.min(row);
                bounds.1 = bounds.1.min(column);
                bounds.2 = bounds.2.max(row + 1);
                bounds.3 = bounds.3.max(column + 1);
            }
        }
    }
    if bounds.0 == usize::MAX {
        return Err("KLGA is outside the GOES CONUS grid".into());
    }
    let patch_width = bounds.3 - bounds.1;
    let patch_height = bounds.2 - bounds.0;
    let crop = |data: &[f64]| {
        let mut out = Vec::with_capacity(patch_width * patch_height);
        for row in bounds.0..bounds.2 {
            out.extend_from_slice(&data[row * width + bounds.1..row * width + bounds.3]);
        }
        out
    };
    let patch_values = crop(&values);
    let patch_latitude = crop(&latitude);
    let patch_longitude = crop(&longitude);
    let patch_dqf = dqf.as_ref().map(|flags| crop(flags));
    let mut summaries = BTreeMap::new();
    for radius in RADII_KM {
        for sector in SECTORS {
            summaries.insert(
                (radius, sector),
                summarize(
                    &patch_values,
                    &patch_latitude,
                    &patch_longitude,
                    patch_dqf.as_deref(),
                    radius,
                    sector,
                    clear_sky,
                ),
            );
        }
    }
    Ok(Patch {
        variable: (*variable).to_owned(),
        units,
        width: patch_width,
        height: patch_height,
        values: patch_values,
        latitude: patch_latitude,
        longitude: patch_longitude,
        dqf: patch_dqf,
        summaries,
    })
}

fn read_xarray_f32(file: &NcFile, name: &str) -> Result<Vec<f64>, String> {
    let variable = file.variable(name).map_err(|error| error.to_string())?;
    let scale = variable
        .attribute("scale_factor")
        .and_then(|value| value.value.as_f64())
        .unwrap_or(1.0) as f32;
    let offset = variable
        .attribute("add_offset")
        .and_then(|value| value.value.as_f64())
        .unwrap_or(0.0) as f32;
    let fill = variable
        .attribute("_FillValue")
        .and_then(|value| value.value.as_f64());
    let missing = variable
        .attribute("missing_value")
        .and_then(|value| value.value.as_f64());
    Ok(file
        .read_variable_as_f64(name)
        .map_err(|error| error.to_string())?
        .into_raw_vec_and_offset()
        .0
        .into_iter()
        .map(|raw| {
            if fill == Some(raw) || missing == Some(raw) {
                f64::NAN
            } else {
                f64::from((raw as f32) * scale + offset)
            }
        })
        .collect())
}

fn attribute(variable: &netcdf_reader::NcVariable, name: &str) -> Result<f64, String> {
    variable
        .attribute(name)
        .and_then(|v| v.value.as_f64())
        .ok_or_else(|| format!("GOES projection attribute {name} is missing"))
}

fn project(x: f64, y: f64, h: f64, ratio: f64, major: f64, origin: f64) -> (f64, f64) {
    // xarray exposes the packed GOES scan coordinates as float32. NumPy keeps
    // the array arithmetic in float32 through the latitude calculation, then
    // promotes the longitude subtraction with its scalar origin to float64.
    // Keeping those widths matters for pixels exactly on a radius boundary.
    let (sin_x, cos_x) = (x as f32).sin_cos();
    let (sin_y, cos_y) = (y as f32).sin_cos();
    let a = sin_x.powi(2) + cos_x.powi(2) * (cos_y.powi(2) + (ratio as f32) * sin_y.powi(2));
    let b = ((-2.0 * h) as f32) * cos_x * cos_y;
    let c = (h * h - major * major) as f32;
    let discriminant = b.powi(2) - 4.0_f32 * a * c;
    if discriminant < 0.0 {
        return (f64::NAN, f64::NAN);
    }
    let distance = (-b - discriminant.sqrt()) / (2.0 * a);
    let sx = distance * cos_x * cos_y;
    let sy = -distance * sin_x;
    let sz = distance * cos_x * sin_y;
    let latitude = ((ratio as f32 * sz) / ((h as f32 - sx).hypot(sy)))
        .atan()
        .to_degrees();
    let longitude = (origin - f64::from(sy.atan2(h as f32 - sx))).to_degrees();
    (f64::from(latitude), longitude)
}

fn in_sector(lat: f64, lon: f64, radius: u32, sector: &str) -> bool {
    if !lat.is_finite() || !lon.is_finite() {
        return false;
    }
    let north = (lat - STATION_LATITUDE) * 111.195;
    let east = (lon - STATION_LONGITUDE) * 111.195 * STATION_LATITUDE.to_radians().cos();
    if north.hypot(east) > f64::from(radius) {
        return false;
    }
    match sector {
        "all" => true,
        "north" => north >= east.abs(),
        "south" => -north > east.abs(),
        "east" => east >= north.abs(),
        "west" => -east > north.abs(),
        _ => false,
    }
}

fn summarize(
    values: &[f64],
    latitude: &[f64],
    longitude: &[f64],
    dqf: Option<&[f64]>,
    radius: u32,
    sector: &str,
    clear_sky: bool,
) -> Summary {
    let mut valid = Vec::new();
    let mut total = 0;
    let mut quality_counts = BTreeMap::new();
    for index in 0..values.len() {
        if !in_sector(latitude[index], longitude[index], radius, sector) {
            continue;
        }
        total += 1;
        if values[index].is_finite() {
            valid.push(values[index]);
        }
        if let Some(flags) = dqf {
            if flags[index].is_finite() {
                *quality_counts.entry(flags[index] as i64).or_insert(0) += 1;
            }
        }
    }
    let mut summary = Summary {
        valid_pixel_fraction: if total > 0 {
            valid.len() as f64 / total as f64
        } else {
            0.0
        },
        quality_valid: quality_counts.values().sum(),
        quality_counts,
        ..Summary::default()
    };
    if valid.is_empty() {
        return summary;
    }
    valid.sort_by(f64::total_cmp);
    let mean = valid.iter().sum::<f64>() / valid.len() as f64;
    summary.mean = Some(mean);
    summary.stddev = Some(
        (valid
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / valid.len() as f64)
            .sqrt(),
    );
    summary.p10 = Some(quantile(&valid, 0.1));
    summary.p50 = Some(quantile(&valid, 0.5));
    summary.p90 = Some(quantile(&valid, 0.9));
    summary.max = valid.last().copied();
    if clear_sky {
        let clear = valid.iter().filter(|value| **value >= 2.0).count();
        summary.clear_fraction = Some(clear as f64 / valid.len() as f64);
        summary.cloudy_fraction = Some((valid.len() - clear) as f64 / valid.len() as f64);
    }
    summary
}

fn quantile(values: &[f64], probability: f64) -> f64 {
    let position = probability * (values.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    values[lower] + (values[upper] - values[lower]) * (position - lower as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_and_sectors_match_training_feature_rules() {
        let values = [1.0, 2.0, 3.0, 4.0, f64::NAN];
        let lat = [STATION_LATITUDE; 5];
        let lon = [STATION_LONGITUDE; 5];
        let summary = summarize(&values, &lat, &lon, None, 25, "all", true);
        assert_eq!(summary.mean, Some(2.5));
        assert_eq!(summary.stddev, Some(1.25_f64.sqrt()));
        assert_eq!(summary.p10, Some(1.3));
        assert_eq!(summary.p50, Some(2.5));
        assert_eq!(summary.p90, Some(3.7));
        assert_eq!(summary.valid_pixel_fraction, 0.8);
        assert_eq!(summary.clear_fraction, Some(0.75));
        assert!(!in_sector(f64::NAN, STATION_LONGITUDE, 100, "all"));
        assert!(in_sector(
            STATION_LATITUDE + 0.1,
            STATION_LONGITUDE,
            25,
            "north"
        ));
        assert!(!in_sector(
            STATION_LATITUDE + 0.1,
            STATION_LONGITUDE,
            25,
            "south"
        ));
    }

    #[test]
    fn real_noaa_c13_matches_python_oracle_when_fixture_is_available() {
        let Ok(path) = std::env::var("GOES_C13_PARITY_FILE") else {
            return;
        };
        let patch = decode(Path::new(&path), &["CMI", "Rad"], false).unwrap();
        let summary = patch.summaries.get(&(100, "all")).unwrap();
        // Oracle: existing Python xarray/h5netcdf feature implementation on the
        // G19 2026-08-31 12:51 UTC C13 source object.
        assert_eq!((patch.width, patch.height), (95, 64));
        assert_eq!(summary.quality_valid, 4_829);
        assert!((summary.mean.unwrap() - 266.55852936543056).abs() < 1e-5);
        assert!((summary.stddev.unwrap() - 8.29086355125496).abs() < 1e-5);
        assert_eq!(summary.p50, Some(265.622314453125));
        assert_eq!(summary.valid_pixel_fraction, 1.0);
        if let Ok(existing) = std::env::var("GOES_C13_PYTHON_PATCH") {
            assert!(patch
                .matches_netcdf(Path::new(&existing), "infrared_c13", "ABI-L2-CMIPC")
                .unwrap());
        }
    }

    #[test]
    fn real_noaa_c02_boundary_stays_within_one_pixel_when_fixture_is_available() {
        let Ok(path) = std::env::var("GOES_C02_PARITY_FILE") else {
            return;
        };
        let patch = decode(Path::new(&path), &["CMI", "Rad"], false).unwrap();
        let all = patch.summaries.get(&(50, "all")).unwrap();
        let east = patch.summaries.get(&(50, "east")).unwrap();
        // NumPy float32 cosine and Rust libm differ by one ULP at one radius
        // boundary on this real C02 grid. This checks the bounded result.
        assert_eq!(all.quality_valid, 19_359);
        assert_eq!(east.quality_valid, 4_842);
        assert!((all.mean.unwrap() - 0.21672871122807372).abs() < 3e-4);
        assert!((east.mean.unwrap() - 0.26203468200561925).abs() < 3e-4);
    }
}
