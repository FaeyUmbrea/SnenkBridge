use std::path::Path;

/// Initialise logging to stdout plus a rolling file under the app's config
/// directory. Failures are non-fatal: the app must still start when the log
/// file can't be created — e.g. launched from Finder, where the working
/// directory is `/` and a relative `log/log.log` would fail and abort startup.
pub fn init_logging() {
    use log4rs::append::console::ConsoleAppender;
    use log4rs::append::rolling_file::policy::compound::{
        roll::delete::DeleteRoller, trigger::size::SizeTrigger, CompoundPolicy,
    };
    use log4rs::append::rolling_file::RollingFileAppender;
    use log4rs::config::{Appender, Config, Root};
    use log4rs::encode::pattern::PatternEncoder;

    let pattern = "[{d(%Y-%m-%d %H:%M:%S)} {h({l}):<5.5} {f}:{L}] {m}{n}";

    let stdout = ConsoleAppender::builder()
        .encoder(Box::new(PatternEncoder::new(pattern)))
        .build();
    let mut config =
        Config::builder().appender(Appender::builder().build("stdout", Box::new(stdout)));
    let mut root = Root::builder().appender("stdout");

    // A writable location regardless of the launch CWD:
    // `~/Library/Application Support/SnenkBridge/log` on macOS, the platform
    // config dir elsewhere; falls back to `./log`.
    let log_dir = dirs::config_dir()
        .map(|d| d.join("SnenkBridge").join("log"))
        .unwrap_or_else(|| Path::new("log").to_path_buf());
    let _ = std::fs::create_dir_all(&log_dir);

    let policy = CompoundPolicy::new(
        Box::new(SizeTrigger::new(1024 * 1024)),
        Box::new(DeleteRoller::new()),
    );
    match RollingFileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(pattern)))
        .build(log_dir.join("log.log"), Box::new(policy))
    {
        Ok(roll) => {
            config = config.appender(Appender::builder().build("roll", Box::new(roll)));
            root = root.appender("roll");
        }
        Err(e) => eprintln!("SnenkBridge: file logging disabled: {e}"),
    }

    match config.build(root.build(log::LevelFilter::Info)) {
        Ok(cfg) => {
            if let Err(e) = log4rs::init_config(cfg) {
                eprintln!("SnenkBridge: logging init failed: {e}");
            }
        }
        Err(e) => eprintln!("SnenkBridge: logging config failed: {e}"),
    }
}
