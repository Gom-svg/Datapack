#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    // No durable logs, file paths, panic payloads, or file contents.
    std::panic::set_hook(Box::new(|_| {
        eprintln!("DataPack Desktop encountered an unexpected internal error.")
    }));
    if let Err(error) = run() {
        #[cfg(windows)]
        if std::env::args_os().len() == 1 {
            datapack_desktop::ui::show_error(&error);
        }
        #[cfg(not(windows))]
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 2 && args[0] == "--smoke-test" {
        return datapack_desktop::smoke::run(std::path::Path::new(&args[1]));
    }
    #[cfg(windows)]
    {
        if args.is_empty() {
            return datapack_desktop::ui::run(false);
        }
        if args.len() == 1 && args[0] == "--ui-smoke" {
            return datapack_desktop::ui::run(true);
        }
    }
    Err("The Desktop window requires Windows x86_64. For adapter certification use --smoke-test <new-directory>.".into())
}
