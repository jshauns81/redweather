# RedWeather Improvements

## Safety & Reliability
- [x] Replace `unwrap()` calls in `src/main.rs`, `src/astro.rs`, and `src/gauges.rs` with proper error handling or `expect("...")` to prevent panics.
- [x] Implement proper error propagation in `src/weather.rs` for `geocode_*` functions instead of returning `Option<Location>`.
- [x] Add error logging/reporting in `load_config` (currently fails silently to defaults).
- [x] Handle file I/O errors in `load_cache` and `save_cache` (currently swallowed).

## Testing
- [x] Add unit tests for `src/weather.rs` (JSON parsing, cache logic).
- [ ] Add tests for `src/config.rs` migration logic.
- [ ] Consider adding integration tests for the full flow (mocking the API).

## Code Quality
- [ ] Use `tracing` or `env_logger` for structured logging instead of `eprintln!`.
- [ ] Refactor `src/dashboard.rs` UI building code into smaller, reusable components/functions to improve readability.
