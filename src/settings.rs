//! Command-line options: Quasar's common flags plus Planetfall's own.

use quasar_config::CommonArgs;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Settings {
    pub headless: bool,
    /// Let the autopilot play (headless or windowed).
    pub autoplay: bool,
    /// Headless: simulation ticks to run.
    pub ticks: u64,
    pub seed: u64,
    /// Headless: render the final state to a PNG (needs a GPU, no window).
    pub screenshot: Option<PathBuf>,
    /// Headless screenshots: show this app screen (title, pause, options).
    pub menu: Option<String>,
    /// Render Basalt cover art (900x1200) to a PNG and exit.
    pub cover: Option<PathBuf>,
    pub asset_root: Option<PathBuf>,
    /// Never write save/config files.
    pub no_save: bool,
    /// Start from a new profile in memory (implies --no-save unless --save is given).
    pub fresh: bool,
    /// Windowed: exit after this many rendered frames (smoke tests).
    pub frames: Option<u64>,
    pub resolution: Option<(u32, u32)>,
    pub fullscreen: Option<bool>,
    pub audio: bool,
    pub audio_device: Option<String>,
    /// Diagnostics threshold (trace, debug, info, warn, error). Default: warn.
    pub log_level: quasar_diagnostics::Level,
}

const GAME_HELP: &str = "Planetfall options:
  --autoplay             Let the autopilot play
  --ticks N              Headless: simulation ticks to run (default 3600)
  --seed N               World seed (default: time-based when windowed, 1 headless)
  --screenshot FILE      Headless: render the final frame to FILE (PNG)
  --cover FILE           Render 900x1200 Basalt cover art to FILE and exit
  --menu NAME            With --screenshot: draw the title, pause or options screen
  --asset-root DIR       Load models/sounds/fonts from DIR
  --fresh                Start from a new in-memory profile (no saving)
  --no-save              Never write save or settings files
  --frames N             Windowed: exit after N frames

Data is stored under the install root's data/ directory (or BASALT_* paths).";

impl Settings {
    /// Parse arguments. `Ok(None)` means help/version was printed.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, String> {
        let mut common = CommonArgs::from_env("PLANETFALL").map_err(|e| e.to_string())?;
        common.parse_into(args).map_err(|e| e.to_string())?;
        if common.help {
            print!("{}", common.help("planetfall", GAME_HELP));
            return Ok(None);
        }
        if common.version {
            println!("planetfall {}", env!("CARGO_PKG_VERSION"));
            return Ok(None);
        }
        let headless = common.is_headless();
        let mut settings = Self {
            headless,
            autoplay: false,
            ticks: 3600,
            seed: if headless { 1 } else { time_seed() },
            screenshot: None,
            cover: None,
            menu: None,
            asset_root: None,
            no_save: false,
            fresh: false,
            frames: None,
            resolution: common.resolution.map(|r| (r.width, r.height)),
            fullscreen: common.fullscreen,
            audio: !common.no_audio.unwrap_or(false),
            audio_device: common.audio_device.clone(),
            log_level: match &common.log_level {
                Some(level) => level.parse().map_err(|e| format!("--log-level: {e}"))?,
                None => quasar_diagnostics::Level::Warn,
            },
        };
        let mut rest = common.remaining.into_iter();
        while let Some(arg) = rest.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((f, v)) => (f.to_owned(), Some(v.to_owned())),
                None => (arg.clone(), None),
            };
            let mut value = |name: &str| {
                inline
                    .clone()
                    .or_else(|| rest.next())
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match flag.as_str() {
                "--autoplay" => settings.autoplay = true,
                "--ticks" => settings.ticks = parse_num(&value("--ticks")?)?,
                "--seed" => settings.seed = parse_num(&value("--seed")?)?,
                "--screenshot" => settings.screenshot = Some(value("--screenshot")?.into()),
                "--cover" => settings.cover = Some(value("--cover")?.into()),
                "--menu" => {
                    let name = value("--menu")?;
                    if !["title", "pause", "options"].contains(&name.as_str()) {
                        return Err(format!(
                            "--menu must be title, pause or options, got {name:?}"
                        ));
                    }
                    settings.menu = Some(name);
                }
                "--asset-root" => settings.asset_root = Some(value("--asset-root")?.into()),
                "--fresh" => settings.fresh = true,
                "--no-save" => settings.no_save = true,
                "--frames" => settings.frames = Some(parse_num(&value("--frames")?)?),
                other => return Err(format!("unknown argument: {other} (see --help)")),
            }
        }
        if settings.fresh {
            settings.no_save = true;
        }
        Ok(Some(settings))
    }
}

fn parse_num(s: &str) -> Result<u64, String> {
    s.parse()
        .map_err(|_| format!("expected a number, got {s:?}"))
}

fn time_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Settings {
        Settings::parse(args.iter().map(|s| s.to_string()))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn parses_game_and_common_flags() {
        let s = parse(&[
            "--headless",
            "--autoplay",
            "--ticks=120",
            "--seed",
            "9",
            "--resolution",
            "800x600",
        ]);
        assert!(s.headless && s.autoplay);
        assert_eq!((s.ticks, s.seed), (120, 9));
        assert_eq!(s.resolution, Some((800, 600)));
        assert!(!s.no_save);
        assert!(parse(&["--fresh"]).no_save);
    }

    #[test]
    fn rejects_unknown_flags() {
        assert!(Settings::parse(["--nope".to_string()]).is_err());
    }
}
