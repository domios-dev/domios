use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone};

pub(super) struct Calendar<Tz>(pub Tz);

pub(super) struct DayWindow {
    pub date: NaiveDate,
    pub start_ms: i64,
    pub end_ms: i64,
}

impl<Tz: TimeZone> Calendar<Tz> {
    pub fn at(&self, ms: i64) -> Result<DateTime<Tz>, String> {
        self.0
            .timestamp_millis_opt(ms)
            .single()
            .ok_or_else(|| format!("invalid calendar timestamp: {ms}"))
    }

    pub fn start(&self, date: NaiveDate) -> Result<i64, String> {
        let midnight = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| format!("invalid calendar date: {date}"))?;
        // Choose the earliest occurrence, or the first valid instant after a midnight gap.
        for second in 0..86_400 {
            let time = midnight + Duration::seconds(second);
            if let Some(start) = self.0.from_local_datetime(&time).earliest() {
                return Ok(start.timestamp_millis());
            }
        }
        Err(format!("local civil date does not exist: {date}"))
    }

    pub fn start_of_day(&self, ms: i64) -> Result<i64, String> {
        self.start(self.at(ms)?.date_naive())
    }

    pub fn shift_start(&self, ms: i64, days: i64) -> Result<i64, String> {
        let date = self.at(ms)?.date_naive();
        let shifted = date
            .checked_add_signed(Duration::days(days))
            .ok_or_else(|| format!("calendar date overflow: {date}"))?;
        self.start(shifted)
    }

    pub fn windows(&self, from_ms: i64, until_ms: i64) -> Result<Vec<DayWindow>, String> {
        let mut date = self.at(from_ms)?.date_naive();
        let mut start_ms = self.start(date)?;
        let mut windows = Vec::new();
        while start_ms < until_ms {
            let next_date = date
                .succ_opt()
                .ok_or_else(|| format!("calendar date overflow: {date}"))?;
            let next_ms = self.start(next_date)?;
            if next_ms <= start_ms {
                return Err(format!("non-increasing calendar boundary: {next_date}"));
            }
            windows.push(DayWindow {
                date,
                start_ms,
                end_ms: next_ms.min(until_ms),
            });
            date = next_date;
            start_ms = next_ms;
        }
        Ok(windows)
    }

    pub fn future_spend(
        &self,
        now_ms: i64,
        cycle_end_ms: i64,
        dow_estimates: &[f64; 7],
    ) -> Result<f64, String> {
        let tomorrow = self.shift_start(now_ms, 1)?;
        Ok(self
            .windows(tomorrow, cycle_end_ms)?
            .iter()
            .map(|day| dow_estimates[day.date.weekday().num_days_from_sunday() as usize])
            .sum())
    }
}
