use indicatif::{ProgressBar, ProgressStyle};

const PROGRESS_THRESHOLD: u64 = 10;

pub struct ProgressReporter {
    bar: Option<ProgressBar>,
}

impl ProgressReporter {
    pub fn new(total: u64, enabled: bool) -> Self {
        if !enabled || total < PROGRESS_THRESHOLD {
            return Self { bar: None };
        }

        let bar = ProgressBar::new(total);
        bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );
        Self { bar: Some(bar) }
    }

    pub fn set_message(&self, msg: &str) {
        if let Some(ref bar) = self.bar {
            bar.set_message(msg.to_string());
        }
    }

    pub fn inc(&self) {
        if let Some(ref bar) = self.bar {
            bar.inc(1);
        }
    }

    pub fn finish(&self) {
        if let Some(ref bar) = self.bar {
            bar.finish_and_clear();
        }
    }
}
