//! HRRR GRIB2 decoding and KLGA feature summaries. Source fixtures are checked
//! against the existing cfgrib environmental feature implementation.

use std::{collections::BTreeMap, f64::consts::PI, io::Cursor};

use grib::{Grib2SubmessageDecoder, GridDefinitionTemplateValues};

const STATION_LAT: f64 = 40.7769;
const STATION_LON: f64 = -73.8740;
pub const FIELDS: [&str; 9] = [
    "temperature_2m",
    "dew_point_2m",
    "wind_u_10m",
    "wind_v_10m",
    "total_cloud_cover",
    "downward_shortwave_radiation",
    "boundary_layer_height",
    "accumulated_precipitation",
    "composite_reflectivity",
];
pub const RADII: [u32; 3] = [25, 50, 100];
pub const SECTORS: [&str; 5] = ["all", "north", "south", "east", "west"];

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub mean: Option<f64>,
    pub stddev: Option<f64>,
    pub maximum: Option<f64>,
    pub valid_fraction: f64,
}

#[derive(Debug)]
pub struct DecodedPatch {
    pub summaries: BTreeMap<(u32, &'static str), BTreeMap<&'static str, Summary>>,
    pub fields_present: Vec<&'static str>,
    pub units: BTreeMap<&'static str, &'static str>,
}

#[derive(Clone, Copy)]
struct LambertGrid {
    ni: usize,
    nj: usize,
    radius: f64,
    n: f64,
    f: f64,
    rho0: f64,
    lon0: f64,
    x1: f64,
    y1: f64,
    dx: f64,
    dy: f64,
}

impl LambertGrid {
    fn from_template(value: &grib::def::grib2::template::Template3_30) -> Result<Self, String> {
        let (major, minor) = value
            .earth_shape
            .radii()
            .ok_or("unknown HRRR earth shape")?;
        if (major - minor).abs() > 0.001
            || value.projection_centre.0 != 0
            || value.scanning_mode.0 != 64
            || value.latin1 != value.latin2
        {
            return Err("unsupported HRRR Lambert grid geometry".into());
        }
        let lat0 = f64::from(value.lad) * 1e-6_f64.to_radians();
        let lon0 = f64::from(value.lov) * 1e-6_f64.to_radians();
        let lat1 = f64::from(value.first_point_lat) * 1e-6_f64.to_radians();
        let lon1 = f64::from(value.first_point_lon) * 1e-6_f64.to_radians();
        let n = lat0.sin();
        let f = lat0.cos() * (PI / 4.0 + lat0 / 2.0).tan().powf(n) / n;
        let rho = |lat: f64| major * f / (PI / 4.0 + lat / 2.0).tan().powf(n);
        let rho0 = rho(lat0);
        let theta1 = n * (lon1 - lon0);
        let rho1 = rho(lat1);
        Ok(Self {
            ni: value.ni as usize,
            nj: value.nj as usize,
            radius: major,
            n,
            f,
            rho0,
            lon0,
            x1: rho1 * theta1.sin(),
            y1: rho0 - rho1 * theta1.cos(),
            dx: f64::from(value.dx) * 1e-3,
            dy: f64::from(value.dy) * 1e-3,
        })
    }

    fn coordinate(&self, index: usize) -> (f64, f64) {
        let i = index % self.ni;
        let j = index / self.ni;
        let x = self.x1 + i as f64 * self.dx;
        let y = self.y1 + j as f64 * self.dy;
        let rho = x.hypot(self.rho0 - y);
        let lat = 2.0 * (self.radius * self.f / rho).powf(1.0 / self.n).atan() - PI / 2.0;
        let lon = self.lon0 + x.atan2(self.rho0 - y) / self.n;
        let lon_deg = lon.to_degrees();
        (
            lat.to_degrees(),
            (lon_deg + 180.0).rem_euclid(360.0) - 180.0,
        )
    }
}

fn field_for(
    discipline: u8,
    category: u8,
    number: u8,
    surface: u8,
    level: f64,
) -> Option<(&'static str, &'static str)> {
    if discipline != 0 {
        return None;
    }
    match (category, number, surface) {
        (0, 0, 103) if level == 2.0 => Some((FIELDS[0], "K")),
        (0, 6, 103) if level == 2.0 => Some((FIELDS[1], "K")),
        (2, 2, 103) if level == 10.0 => Some((FIELDS[2], "m s**-1")),
        (2, 3, 103) if level == 10.0 => Some((FIELDS[3], "m s**-1")),
        (6, 1, 10) => Some((FIELDS[4], "%")),
        (4, 7, 1) => Some((FIELDS[5], "W m**-2")),
        (3, 18, 1) => Some((FIELDS[6], "m")),
        (1, 8, 1) => Some((FIELDS[7], "kg m**-2")),
        (16, 196, 10) => Some((FIELDS[8], "dB")),
        _ => None,
    }
}

fn selected_positions(grid: LambertGrid) -> Result<(Vec<usize>, usize), String> {
    let mut positions = Vec::new();
    let mut nearest = None;
    for idx in 0..grid.ni * grid.nj {
        let (lat, lon) = grid.coordinate(idx);
        let north = (lat - STATION_LAT) * 111.195;
        let east = (lon - STATION_LON) * 111.195 * STATION_LAT.to_radians().cos();
        let d2 = north * north + east * east;
        if d2 <= 10000.0 {
            positions.push(idx);
        }
        if nearest.is_none_or(|(_, best)| d2 < best) {
            nearest = Some((idx, d2));
        }
    }
    let point = nearest.ok_or("empty HRRR grid")?.0;
    if positions.is_empty() || !positions.contains(&point) {
        return Err("KLGA lies outside the HRRR grid".into());
    }
    Ok((positions, point))
}

fn summarize(values: &[f32], indexes: &[usize], scale: f64) -> Summary {
    let valid: Vec<f64> = indexes
        .iter()
        .filter_map(|i| {
            let v = f64::from(values[*i]) / scale;
            v.is_finite().then_some(v)
        })
        .collect();
    if valid.is_empty() {
        return Summary {
            mean: None,
            stddev: None,
            maximum: None,
            valid_fraction: 0.0,
        };
    }
    let mean = valid.iter().sum::<f64>() / valid.len() as f64;
    let variance = valid.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / valid.len() as f64;
    let fraction = valid.len() as f64 / indexes.len() as f64;
    Summary {
        mean: Some(mean),
        stddev: Some(variance.sqrt()),
        maximum: valid.into_iter().reduce(f64::max),
        valid_fraction: fraction,
    }
}

fn region_indexes(
    grid: LambertGrid,
    positions: &[usize],
    point: usize,
) -> BTreeMap<(u32, &'static str), Vec<usize>> {
    let mut out = BTreeMap::new();
    out.insert((0, "all"), vec![point]);
    for radius in RADII {
        for sector in SECTORS {
            let indexes = positions
                .iter()
                .copied()
                .filter(|idx| {
                    let (lat, lon) = grid.coordinate(*idx);
                    let north = (lat - STATION_LAT) * 111.195;
                    let east = (lon - STATION_LON) * 111.195 * STATION_LAT.to_radians().cos();
                    let distance = north.hypot(east);
                    distance <= f64::from(radius)
                        && match sector {
                            "all" => true,
                            "north" => north >= east.abs(),
                            "south" => -north > east.abs(),
                            "east" => east >= north.abs(),
                            "west" => -east > north.abs(),
                            _ => false,
                        }
                })
                .collect();
            out.insert((radius, sector), indexes);
        }
    }
    out
}

pub fn decode(bytes: &[u8]) -> Result<DecodedPatch, String> {
    let source = grib::from_reader(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut grid = None;
    let mut values: BTreeMap<&'static str, Vec<f32>> = BTreeMap::new();
    let mut units = BTreeMap::new();
    for (_, message) in source.iter() {
        let Some(category) = message.prod_def().parameter_category() else {
            continue;
        };
        let Some(number) = message.prod_def().parameter_number() else {
            continue;
        };
        let Some((surface, _)) = message.prod_def().fixed_surfaces() else {
            continue;
        };
        let Some((field, unit)) = field_for(
            message.indicator().discipline,
            category,
            number,
            surface.surface_type,
            surface.value(),
        ) else {
            continue;
        };
        let GridDefinitionTemplateValues::Template30(template) =
            GridDefinitionTemplateValues::try_from(message.grid_def())
                .map_err(|e| e.to_string())?
        else {
            return Err("HRRR field is not on a Lambert 3.30 grid".into());
        };
        let candidate = LambertGrid::from_template(&template)?;
        if grid.is_some_and(|existing: LambertGrid| {
            existing.ni != candidate.ni
                || existing.nj != candidate.nj
                || (existing.x1 - candidate.x1).abs() > 0.001
        }) {
            return Err("HRRR subset mixed incompatible grids".into());
        }
        grid = Some(candidate);
        let decoded: Vec<f32> = Grib2SubmessageDecoder::from(message)
            .map_err(|e| e.to_string())?
            .dispatch()
            .map_err(|e| e.to_string())?
            .collect();
        if decoded.len() != candidate.ni * candidate.nj {
            return Err("HRRR decoded field size mismatch".into());
        }
        // cfgrib selects the first accumulation interval when NOAA publishes
        // both cumulative and trailing-hour APCP for the same valid time.
        if field == "accumulated_precipitation" && values.contains_key(field) {
            continue;
        }
        values.insert(field, decoded);
        units.insert(field, unit);
    }
    let grid = grid.ok_or("HRRR subset contained no recognized fields")?;
    let cloud_fraction_needed = values.get("total_cloud_cover").is_some_and(|cloud| {
        cloud
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(f32::NEG_INFINITY, f32::max)
            > 1.5
    });
    let cloud_scale = if cloud_fraction_needed { 100.0 } else { 1.0 };
    let (positions, point) = selected_positions(grid)?;
    let regions = region_indexes(grid, &positions, point);
    let summaries = regions
        .into_iter()
        .map(|(region, indexes)| {
            let stats = values
                .iter()
                .map(|(name, vals)| {
                    (
                        *name,
                        summarize(
                            vals,
                            &indexes,
                            if *name == "total_cloud_cover" {
                                cloud_scale
                            } else {
                                1.0
                            },
                        ),
                    )
                })
                .collect();
            (region, stats)
        })
        .collect();
    Ok(DecodedPatch {
        summaries,
        fields_present: values.keys().copied().collect(),
        units,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrrr_lambert_coordinates_match_cfgrib_oracle() {
        let grid = LambertGrid {
            ni: 1799,
            nj: 1059,
            radius: 6_371_229.0,
            n: 38.5_f64.to_radians().sin(),
            f: 38.5_f64.to_radians().cos()
                * (PI / 4.0 + 38.5_f64.to_radians() / 2.0)
                    .tan()
                    .powf(38.5_f64.to_radians().sin())
                / 38.5_f64.to_radians().sin(),
            rho0: 0.0,
            lon0: 262.5_f64.to_radians(),
            x1: 0.0,
            y1: 0.0,
            dx: 3000.0,
            dy: 3000.0,
        };
        let mut grid = grid;
        grid.rho0 =
            grid.radius * grid.f / (PI / 4.0 + 38.5_f64.to_radians() / 2.0).tan().powf(grid.n);
        let rho1 = grid.radius * grid.f
            / (PI / 4.0 + 21.138123_f64.to_radians() / 2.0)
                .tan()
                .powf(grid.n);
        let theta = grid.n * (237.280472_f64.to_radians() - grid.lon0);
        grid.x1 = rho1 * theta.sin();
        grid.y1 = grid.rho0 - rho1 * theta.cos();
        for (i, j, lat, lon) in [
            (0, 0, 21.138123, -122.719528),
            (500, 500, 36.92301201528325, -111.01427694126443),
            (1140, 750, 44.13157226399325, -88.47821584906995),
            (1798, 1058, 47.84219502248864, -60.91719277183785),
        ] {
            let actual = grid.coordinate(j * grid.ni + i);
            assert!((actual.0 - lat).abs() < 1e-9);
            assert!((actual.1 - lon).abs() < 1e-9);
        }
    }

    #[test]
    fn summary_uses_population_stddev_and_nan_exclusion() {
        let s = summarize(&[1.0, 3.0, f32::NAN, 5.0], &[0, 1, 2, 3], 1.0);
        assert_eq!(s.mean, Some(3.0));
        assert!((s.stddev.unwrap() - (8.0_f64 / 3.0).sqrt()).abs() < 1e-12);
        assert_eq!(s.maximum, Some(5.0));
        assert_eq!(s.valid_fraction, 0.75);
    }

    #[test]
    fn real_noaa_subset_matches_cfgrib_numeric_oracle_when_available() {
        let Ok(path) = std::env::var("HRRR_GRIB_PARITY_FIXTURE") else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.fields_present.len(), 9);
        let region = &decoded.summaries[&(25, "all")];
        for (name, expected, tolerance) in [
            ("temperature_2m", 278.0201120684224, 1e-9),
            ("dew_point_2m", 274.50523861652147, 1e-9),
            ("wind_u_10m", 0.03736127686390679, 1e-9),
            ("wind_v_10m", -1.0792040671071699, 1e-9),
            ("total_cloud_cover", 0.9131336405529953, 1e-9),
            ("downward_shortwave_radiation", 159.8188941357872, 1e-5),
            ("boundary_layer_height", 453.38343888383855, 1e-9),
            ("accumulated_precipitation", 0.006589861603857185, 1e-9),
            ("composite_reflectivity", -9.951900921658986, 1e-9),
        ] {
            assert!(
                (region[name].mean.unwrap() - expected).abs() < tolerance,
                "{name}"
            );
        }
    }
}
