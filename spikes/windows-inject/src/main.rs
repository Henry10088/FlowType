#[cfg(not(target_os = "windows"))]
compile_error!("flowtype-windows-inject-spike only runs on Windows");

mod diff;
mod inject;
mod self_test;
mod target;

use std::env;
use std::error::Error;
use std::io::{self, Write};
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

use diff::plan_transition;
use target::TargetWindow;

const DEFAULT_FOCUS_DELAY_SECONDS: u64 = 5;
const DEFAULT_STEP_DELAY_MS: u64 = 1200;

#[derive(Debug)]
struct Config {
    scenario: Scenario,
    focus_delay_seconds: u64,
    step_delay_ms: u64,
    dry_run: bool,
    self_test: bool,
}

#[derive(Clone, Copy, Debug)]
enum Scenario {
    Unicode,
    Rewrite,
    Multiline,
}

impl Scenario {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "unicode" => Some(Self::Unicode),
            "rewrite" => Some(Self::Rewrite),
            "multiline" => Some(Self::Multiline),
            _ => None,
        }
    }

    fn snapshots(self) -> &'static [&'static str] {
        match self {
            Self::Unicode => &[
                "你好",
                "你好，Windows",
                "你好，Windows 🙂",
                "你好，Windows 🙂 café",
            ],
            Self::Rewrite => &[
                "豆包正在识别",
                "豆包正在识别语音",
                "豆包正在识别文本",
                "豆包语音识别文本。",
            ],
            Self::Multiline => &["第一行", "第一行\n第二行", "第一行\n第二行\n第三行🙂"],
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Unicode => "unicode",
            Self::Rewrite => "rewrite",
            Self::Multiline => "multiline",
        }
    }
}

fn main() -> ExitCode {
    match parse_config().and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("错误：{error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_config() -> Result<Config, Box<dyn Error>> {
    let mut scenario = Scenario::Unicode;
    let mut focus_delay_seconds = DEFAULT_FOCUS_DELAY_SECONDS;
    let mut step_delay_ms = DEFAULT_STEP_DELAY_MS;
    let mut dry_run = false;
    let mut self_test = false;

    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--scenario" => {
                let value = arguments.next().ok_or("--scenario 缺少值")?;
                scenario =
                    Scenario::parse(&value).ok_or("未知场景；可选 unicode、rewrite、multiline")?;
            }
            "--focus-delay-seconds" => {
                focus_delay_seconds = parse_number("--focus-delay-seconds", arguments.next())?;
            }
            "--step-delay-ms" => {
                step_delay_ms = parse_number("--step-delay-ms", arguments.next())?;
            }
            "--dry-run" => dry_run = true,
            "--self-test" => self_test = true,
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("未知参数：{argument}；使用 --help 查看说明").into()),
        }
    }

    Ok(Config {
        scenario,
        focus_delay_seconds,
        step_delay_ms,
        dry_run,
        self_test,
    })
}

fn parse_number(name: &str, value: Option<String>) -> Result<u64, Box<dyn Error>> {
    value
        .ok_or_else(|| format!("{name} 缺少值"))?
        .parse::<u64>()
        .map_err(|_| format!("{name} 必须是非负整数").into())
}

fn run(config: Config) -> Result<(), Box<dyn Error>> {
    if config.self_test {
        return self_test::run();
    }

    println!("说写 Windows 注入验证 · 场景 {}", config.scenario.name());

    if matches!(config.scenario, Scenario::Multiline) {
        println!("警告：换行会作为 Enter 注入。不要在终端或其他会执行 Enter 的窗口中测试。");
    }

    if config.dry_run {
        return print_dry_run(config.scenario);
    }

    println!(
        "请在 {} 秒内把光标放到目标窗口；倒计时结束后程序会锁定该窗口。",
        config.focus_delay_seconds
    );
    countdown(config.focus_delay_seconds)?;

    let target = TargetWindow::capture_foreground().ok_or("无法获取前台窗口")?;
    println!(
        "已锁定窗口：{} [0x{:X}]",
        target.title(),
        target.raw_value()
    );

    let mut previous = String::new();
    for (index, snapshot) in config.scenario.snapshots().iter().enumerate() {
        if !target.is_foreground() {
            return Err("目标窗口已失去前台，已停止注入；没有向新窗口发送内容".into());
        }

        let transition = plan_transition(&previous, snapshot);
        println!(
            "步骤 {}/{}：退格 {} 次，注入 {} 个 Unicode 字符",
            index + 1,
            config.scenario.snapshots().len(),
            transition.backspaces,
            transition.insert.chars().count(),
        );

        inject::send_backspaces(transition.backspaces)?;
        inject::send_text(&transition.insert)?;
        previous.clear();
        previous.push_str(snapshot);

        if index + 1 < config.scenario.snapshots().len() {
            thread::sleep(Duration::from_millis(config.step_delay_ms));
        }
    }

    println!("场景完成。程序不会自动清理已注入的测试文字。");
    Ok(())
}

fn countdown(seconds: u64) -> io::Result<()> {
    for remaining in (1..=seconds).rev() {
        print!("{remaining} ");
        io::stdout().flush()?;
        thread::sleep(Duration::from_secs(1));
    }
    println!();
    Ok(())
}

fn print_dry_run(scenario: Scenario) -> Result<(), Box<dyn Error>> {
    let mut previous = "";
    for (index, snapshot) in scenario.snapshots().iter().enumerate() {
        let transition = plan_transition(previous, snapshot);
        println!(
            "步骤 {}：退格 {} 次，追加 {:?}",
            index + 1,
            transition.backspaces,
            transition.insert,
        );
        previous = snapshot;
    }
    Ok(())
}

fn print_help() {
    println!(
        "\
用法：flowtype-windows-inject-spike [选项]\n\
\n\
选项：\n\
  --scenario <名称>             unicode（默认）、rewrite、multiline\n\
  --focus-delay-seconds <秒>    切换到目标窗口的倒计时，默认 5\n\
  --step-delay-ms <毫秒>        两个完整状态之间的间隔，默认 1200\n\
  --dry-run                     只显示 diff，不发送输入\n\
  --self-test                   在标准 Win32 编辑控件中自动验证 SendInput\n\
  -h, --help                    显示帮助\n"
    );
}
