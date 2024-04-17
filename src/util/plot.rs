use anyhow::anyhow;
use chrono::{DateTime, Utc};
use itertools::{Itertools, MinMaxResult};
use plotters::prelude::*;

pub fn plot_timeseries_svg(
    label: &str,
    entries: Vec<(DateTime<Utc>, f64)>,
) -> anyhow::Result<String> {
    if let MinMaxResult::MinMax(min_y, max_y) = entries.iter().map(|r| r.1).minmax() {
        let mut buf = String::new();

        let y_diff = max_y - min_y;
        let min_y = min_y - 0.05 * y_diff - 0.1;
        let max_y = max_y + 0.05 * y_diff + 0.1;

        let first_entry = entries.first().ok_or(anyhow!("Invalid input for plot"))?;
        let last_entry = entries.last().ok_or(anyhow!("Invalid input for plot"))?;

        {
            let root = SVGBackend::with_string(&mut buf, (480, 240)).into_drawing_area();
            let mut chart = ChartBuilder::on(&root)
                .margin(5)
                .x_label_area_size(20)
                .y_label_area_size(30)
                .build_cartesian_2d(first_entry.0..last_entry.0, min_y..max_y)?;

            chart
                .configure_mesh()
                .y_labels(5)
                .x_labels(4)
                .disable_mesh()
                .draw()?;

            chart
                .draw_series(LineSeries::new(entries, &RED))?
                .label(label);

            root.present()?;
        }

        Ok(buf)
    } else {
        Err(anyhow::anyhow!("Cannot create plot"))
    }
}
