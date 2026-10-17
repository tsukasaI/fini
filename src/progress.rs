use indicatif::{ProgressBar, ProgressStyle};

const PROGRESS_THRESHOLD: u64 = 10;

pub fn progress_bar(total: u64, enabled: bool) -> ProgressBar {
    if !enabled || total < PROGRESS_THRESHOLD {
        return ProgressBar::hidden();
    }

    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    bar
}
