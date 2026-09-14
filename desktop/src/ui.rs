//! Native Windows shell. The only unsafe code is the Win32 boundary in this module.
//!
//! Handles and the RefCell below belong to the message-loop thread. The boxed
//! state outlives its window. Reentrant notifications use try_borrow_mut, so a
//! nested native dialog/message cannot alias a mutable UI reference. Workers
//! never receive handles or references to controls and are polled non-blockingly.
use std::cell::RefCell;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::Arc;

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::adapter::{CompressionChoice, Controller, Job};
use crate::model::*;
use crate::presentation;

const DATA: i32 = 101;
const ARCHIVE: i32 = 102;
const BROWSE: i32 = 103;
const DESTINATION: i32 = 104;
const ORIGINAL: i32 = 105;
const ANALYZE: i32 = 106;
const EXECUTE: i32 = 107;
const CANCEL: i32 = 108;
const DETAILS: i32 = 109;
const OPEN_ARCHIVE: i32 = 110;
const HELP: i32 = 111;
const INPUT: i32 = 112;
const OUTPUT: i32 = 113;
const AGAINST: i32 = 114;
const METHOD: i32 = 115;
const TIMER: usize = 1;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

struct Control {
    handle: HWND,
    id: i32,
}
struct Window {
    controller: Controller,
    handle: HWND,
    controls: Vec<Control>,
    fonts: Vec<HFONT>,
    kind: FileKind,
    details: bool,
    close_after_work: bool,
    selected_path: Option<PathBuf>,
    title: HWND,
    summary: HWND,
    info: HWND,
    status: HWND,
    bar: HWND,
    option_label: HWND,
    smoke: bool,
    smoke_ticks: u32,
    smoke_passed: bool,
}

impl Window {
    fn new(smoke: bool) -> Self {
        Self {
            controller: Controller::default(),
            handle: null_mut(),
            controls: Vec::new(),
            fonts: Vec::new(),
            kind: FileKind::Data,
            details: false,
            close_after_work: false,
            selected_path: None,
            title: null_mut(),
            summary: null_mut(),
            info: null_mut(),
            status: null_mut(),
            bar: null_mut(),
            option_label: null_mut(),
            smoke,
            smoke_ticks: 0,
            smoke_passed: false,
        }
    }

    fn control(&self, id: i32) -> HWND {
        self.controls
            .iter()
            .find(|c| c.id == id)
            .map_or(null_mut(), |c| c.handle)
    }

    // SAFETY: called only by the UI thread with a live parent and Win32-owned controls.
    unsafe fn add(&mut self, class: &str, text: &str, id: i32, style: u32) -> Result<HWND, String> {
        let handle = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            0,
            0,
            self.handle,
            id as usize as HMENU,
            GetModuleHandleW(null()),
            null(),
        );
        if handle.is_null() {
            return Err("A native control could not be created.".into());
        }
        self.controls.push(Control { handle, id });
        Ok(handle)
    }

    unsafe fn create(&mut self) -> Result<(), String> {
        self.add("STATIC", "DataPack", 201, 0)?;
        self.add(
            "STATIC",
            "Reliable compression. Exact bytes. All on this computer.",
            202,
            0,
        )?;
        for (id, label) in [
            (DATA, "&Compress data"),
            (ARCHIVE, "&Validate && restore"),
            (HELP, "&Help"),
        ] {
            self.add(
                "BUTTON",
                label,
                id,
                WS_TABSTOP
                    | if id == HELP {
                        0
                    } else {
                        BS_AUTORADIOBUTTON as u32 | BS_PUSHLIKE as u32
                    },
            )?;
        }
        self.add("STATIC", "Selected file", 203, 0)?;
        self.add(
            "EDIT",
            "Choose a file to get started",
            INPUT,
            WS_BORDER | ES_READONLY as u32 | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
        )?;
        self.add("BUTTON", "&Browse…", BROWSE, WS_TABSTOP)?;
        self.info = self.add(
            "STATIC",
            "No file selected. DataPack does not preview or buffer your file.",
            204,
            0,
        )?;
        self.add(
            "STATIC",
            "Destination — new filename (existing files are protected)",
            205,
            0,
        )?;
        self.add(
            "EDIT",
            "",
            OUTPUT,
            WS_BORDER | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
        )?;
        self.add("BUTTON", "Choose…", DESTINATION, WS_TABSTOP)?;
        self.option_label = self.add("STATIC", "Compression method", 206, 0)?;
        self.add(
            "COMBOBOX",
            "",
            METHOD,
            WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL,
        )?;
        for label in [
            "Automatic / Recommended",
            "Chunked — bounded in-flight data",
        ] {
            SendMessageW(
                self.control(METHOD),
                CB_ADDSTRING,
                0,
                wide(label).as_ptr() as isize,
            );
        }
        SendMessageW(self.control(METHOD), CB_SETCURSEL, 0, 0);
        self.add(
            "EDIT",
            "",
            AGAINST,
            WS_BORDER | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
        )?;
        self.add("BUTTON", "Browse…", ORIGINAL, WS_TABSTOP)?;
        self.add("BUTTON", "&Analyze", ANALYZE, WS_TABSTOP)?;
        self.add(
            "BUTTON",
            "Compress file",
            EXECUTE,
            WS_TABSTOP | BS_DEFPUSHBUTTON as u32,
        )?;
        self.title = self.add("STATIC", "", 207, 0)?;
        self.summary = self.add(
            "EDIT",
            "",
            208,
            ES_MULTILINE as u32
                | ES_READONLY as u32
                | ES_AUTOVSCROLL as u32
                | WS_VSCROLL
                | WS_TABSTOP,
        )?;
        self.status = self.add("STATIC", "Ready", 209, 0)?;
        self.bar = self.add("msctls_progress32", "", 210, PBS_SMOOTH)?;
        SendMessageW(self.bar, PBM_SETRANGE32, 0, 1000);
        for (id, label) in [
            (CANCEL, "Cancel operation"),
            (DETAILS, "Show &details"),
            (OPEN_ARCHIVE, "Validate or restore result"),
        ] {
            self.add("BUTTON", label, id, WS_TABSTOP)?;
        }
        self.add(
            "STATIC",
            concat!(
                env!("CARGO_PKG_VERSION"),
                " · Internal development build · No uploads or telemetry"
            ),
            211,
            0,
        )?;
        self.set_fonts();
        self.refresh();
        Ok(())
    }

    unsafe fn set_fonts(&mut self) {
        let dpi = GetDpiForWindow(self.handle).max(96) as i32;
        let mut fonts = Vec::new();
        for (size, weight) in [(16, 400), (30, 600), (22, 600), (13, 400)] {
            let font = CreateFontW(
                -size * dpi / 96,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                wide("Segoe UI").as_ptr(),
            );
            fonts.push(font);
        }
        for control in &self.controls {
            let font = match control.id {
                201 => fonts[1],
                207 => fonts[2],
                211 => fonts[3],
                _ => fonts[0],
            };
            SendMessageW(control.handle, WM_SETFONT, font as usize, 1);
        }
        for font in self.fonts.drain(..) {
            if !font.is_null() {
                DeleteObject(font);
            }
        }
        self.fonts = fonts;
    }

    unsafe fn layout(&self) {
        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(self.handle, &mut rect);
        let scale = GetDpiForWindow(self.handle).max(96) as f64 / 96.0;
        let w = (rect.right as f64 / scale) as i32;
        let h = (rect.bottom as f64 / scale) as i32;
        let position = |id, x, y, width, height| {
            MoveWindow(
                self.control(id),
                (x as f64 * scale) as i32,
                (y as f64 * scale) as i32,
                (width as f64 * scale) as i32,
                (height as f64 * scale) as i32,
                1,
            );
        };
        position(201, 30, 20, w - 60, 42);
        position(202, 32, 67, w - 64, 24);
        position(DATA, 30, 105, 185, 36);
        position(ARCHIVE, 228, 105, 210, 36);
        position(HELP, w - 125, 105, 95, 36);
        position(203, 30, 150, w - 60, 22);
        position(INPUT, 30, 177, w - 205, 31);
        position(BROWSE, w - 160, 177, 130, 31);
        position(204, 30, 216, w - 60, 24);
        position(205, 30, 251, w - 60, 22);
        position(OUTPUT, 30, 278, w - 205, 31);
        position(DESTINATION, w - 160, 278, 130, 31);
        position(206, 30, 322, w - 60, 22);
        position(METHOD, 30, 349, w - 205, 160);
        position(AGAINST, 30, 349, w - 205, 31);
        position(ORIGINAL, w - 160, 349, 130, 31);
        position(ANALYZE, 30, 393, 190, 36);
        position(EXECUTE, 235, 393, 230, 36);
        position(207, 30, 451, w - 60, 32);
        position(208, 30, 493, w - 60, (h - 633).max(36));
        position(209, 30, h - 127, w - 240, 38);
        position(CANCEL, w - 200, h - 128, 170, 33);
        position(210, 30, h - 79, w - 60, 13);
        position(DETAILS, 30, h - 49, 175, 30);
        position(OPEN_ARCHIVE, 220, h - 49, 240, 30);
        position(211, 480, h - 40, w - 510, 24);
    }

    unsafe fn set_text(handle: HWND, value: &str) {
        // Avoid resetting scroll/selection when the presentation is unchanged.
        let mut current = vec![0_u16; GetWindowTextLengthW(handle).max(0) as usize + 1];
        GetWindowTextW(handle, current.as_mut_ptr(), current.len() as i32);
        if current != wide(value) {
            SetWindowTextW(handle, wide(value).as_ptr());
        }
    }

    unsafe fn path(&self, id: i32) -> PathBuf {
        let handle = self.control(id);
        let mut text = vec![0_u16; GetWindowTextLengthW(handle).max(0) as usize + 1];
        let length = GetWindowTextW(handle, text.as_mut_ptr(), text.len() as i32);
        PathBuf::from(OsString::from_wide(&text[..length.max(0) as usize]))
    }

    unsafe fn set_path(&self, id: i32, path: &std::path::Path) {
        let text: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        SetWindowTextW(self.control(id), text.as_ptr());
    }

    unsafe fn refresh(&mut self) {
        self.controller.poll();
        if let Some(file) = &self.controller.selected {
            if self.selected_path.as_ref() != Some(&file.path) {
                self.set_path(INPUT, &file.path);
                self.set_path(OUTPUT, &proposed_output(&file.path, file.kind));
                self.selected_path = Some(file.path.clone());
            }
            let format = self
                .controller
                .analysis
                .as_ref()
                .map_or(String::new(), |a| {
                    format!(" · {} · {} delimiter", a.format, a.delimiter)
                });
            Self::set_text(
                self.info,
                &format!("{} ({} bytes){}", bytes(file.bytes), file.bytes, format),
            );
        }
        if self.controller.selected.is_none() && !self.controller.busy() {
            Self::set_text(self.info, "No readable file selected.");
        }
        for (id, kind) in [(DATA, FileKind::Data), (ARCHIVE, FileKind::Archive)] {
            SendMessageW(
                self.control(id),
                BM_SETCHECK,
                if self.kind == kind {
                    BST_CHECKED
                } else {
                    BST_UNCHECKED
                } as usize,
                0,
            );
        }
        let busy = self.controller.busy();
        let selected = self.controller.selected.is_some();
        for id in [
            DATA,
            ARCHIVE,
            BROWSE,
            OUTPUT,
            DESTINATION,
            AGAINST,
            ORIGINAL,
            METHOD,
            HELP,
        ] {
            EnableWindow(self.control(id), (!busy) as i32);
        }
        for id in [ANALYZE, EXECUTE] {
            EnableWindow(self.control(id), (!busy && selected) as i32);
        }
        EnableWindow(
            self.control(CANCEL),
            (busy
                && self.controller.operation() != Some(Operation::Select)
                && !matches!(self.controller.state, State::Cancelling(_))) as i32,
        );
        EnableWindow(self.control(DETAILS), (!busy) as i32);
        ShowWindow(
            self.control(METHOD),
            if self.kind == FileKind::Data {
                SW_SHOW
            } else {
                SW_HIDE
            },
        );
        for id in [AGAINST, ORIGINAL] {
            ShowWindow(
                self.control(id),
                if self.kind == FileKind::Archive {
                    SW_SHOW
                } else {
                    SW_HIDE
                },
            );
        }
        Self::set_text(
            self.option_label,
            if self.kind == FileKind::Data {
                "Compression method (details include resource settings)"
            } else {
                "Original file — optional, for exact source comparison"
            },
        );
        Self::set_text(
            self.control(ANALYZE),
            if self.kind == FileKind::Data {
                "&Analyze"
            } else {
                "Validate archive"
            },
        );
        Self::set_text(
            self.control(EXECUTE),
            if self.kind == FileKind::Data {
                "Compress file"
            } else {
                "Decompress archive"
            },
        );
        ShowWindow(
            self.control(OPEN_ARCHIVE),
            if matches!(self.controller.state, State::CompressComplete(_)) {
                SW_SHOW
            } else {
                SW_HIDE
            },
        );
        let mut view = presentation::view(&self.controller);
        if self.kind == FileKind::Archive && matches!(self.controller.state, State::Idle) {
            view.title = "Check integrity. Restore your data.".into();
            view.body = "Browse to a DataPack archive to validate it or restore your data.\r\n\r\nSupply the original file to check exact source agreement. Validation creates no output.\r\n\r\nRestoration writes to a new destination; existing files stay protected.".into();
        }
        Self::set_text(self.title, &view.title);
        if self.details {
            Self::set_text(self.summary, &format!("{}\r\n\r\nCompression settings\r\nAutomatic uses the existing V1 planner and safety defaults.\r\nChunked uses V2: 8 MiB chunks, 2 workers, 2 in-flight chunks, 16 MiB admission limit. This is not a process memory ceiling.\r\nValidation and decompression use a 512 MiB engine memory limit (not an RSS ceiling).\r\n\r\nNo overwrite or retained partial output is enabled.\r\nNo paths or preferences are persisted by DataPack.", view.details));
        } else {
            Self::set_text(self.summary, &view.body);
        }
        Self::set_text(
            self.control(DETAILS),
            if self.details {
                "Back to summary"
            } else {
                "Show &details"
            },
        );
        Self::set_text(
            self.status,
            &if busy {
                presentation::progress_text(self.controller.progress.as_ref())
            } else {
                "Ready · Your files stay local".into()
            },
        );
        let percent = self.controller.progress.as_ref().and_then(|p| p.percent);
        let marquee = busy && percent.is_none();
        let style = GetWindowLongW(self.bar, GWL_STYLE) as u32;
        SetWindowLongW(
            self.bar,
            GWL_STYLE,
            if marquee {
                style | PBS_MARQUEE
            } else {
                style & !PBS_MARQUEE
            } as i32,
        );
        SendMessageW(self.bar, PBM_SETMARQUEE, marquee as usize, 40);
        SendMessageW(
            self.bar,
            PBM_SETPOS,
            percent.map_or(0, |v| (v * 10.0) as usize),
            0,
        );
        if self.close_after_work && !busy {
            PostMessageW(self.handle, WM_CLOSE, 0, 0);
        }
    }

    unsafe fn start(&mut self, job: Job) {
        self.details = false;
        // Timer polls actual events; it never invents phase progress or a percentage.
        if let Err(problem) = self.controller.start(job, Arc::new(|| {})) {
            self.controller.state = State::Failed(problem);
        }
        self.refresh();
    }

    unsafe fn pick(&self, save: bool, archive: bool, initial: PathBuf) -> Option<PathBuf> {
        let mut buffer = vec![0_u16; 32768];
        let initial: Vec<_> = initial.as_os_str().encode_wide().collect();
        if initial.len() < buffer.len() {
            buffer[..initial.len()].copy_from_slice(&initial);
        }
        let filter = wide(if archive {
            "DataPack archives\0*.dpack\0All files\0*.*\0"
        } else {
            "Data files\0*.csv;*.tsv;*.psv;*.txt\0All files\0*.*\0"
        });
        let title = wide(if save {
            "Choose a NEW filename — DataPack protects existing files"
        } else {
            "Select a file for DataPack"
        });
        let mut dialog: OPENFILENAMEW = std::mem::zeroed();
        dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        dialog.hwndOwner = self.handle;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = buffer.as_mut_ptr();
        dialog.nMaxFile = buffer.len() as u32;
        dialog.lpstrTitle = title.as_ptr();
        dialog.Flags = OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | OFN_DONTADDTORECENT
            | if save { 0 } else { OFN_FILEMUSTEXIST };
        let accepted = if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        };
        if accepted == 0 {
            if CommDlgExtendedError() != 0 {
                MessageBoxW(self.handle, wide("The file dialog could not open. Try again after checking folder permissions.").as_ptr(), wide("DataPack").as_ptr(), MB_OK | MB_ICONERROR);
            }
            return None;
        }
        let length = buffer.iter().position(|v| *v == 0).unwrap_or(buffer.len());
        Some(PathBuf::from(OsString::from_wide(&buffer[..length])))
    }

    unsafe fn command(&mut self, id: i32) {
        if self.controller.busy() && id != CANCEL {
            return;
        }
        match id {
            DATA | ARCHIVE => {
                self.kind = if id == DATA {
                    FileKind::Data
                } else {
                    FileKind::Archive
                };
                self.controller = Controller::default();
                self.selected_path = None;
                self.details = false;
                for id in [INPUT, OUTPUT, AGAINST] {
                    Self::set_text(self.control(id), "");
                }
                Self::set_text(self.info, "No file selected.");
            }
            BROWSE => {
                if let Some(path) = self.pick(false, self.kind == FileKind::Archive, PathBuf::new())
                {
                    self.selected_path = None;
                    self.set_path(INPUT, &path);
                    Self::set_text(self.info, "Reading file metadata…");
                    Self::set_text(self.control(OUTPUT), "");
                    self.start(Job::Select {
                        path,
                        kind: self.kind,
                    });
                }
            }
            DESTINATION => {
                if let Some(path) = self.pick(true, self.kind == FileKind::Data, self.path(OUTPUT))
                {
                    self.set_path(OUTPUT, &path);
                }
            }
            ORIGINAL => {
                if let Some(path) = self.pick(false, false, self.path(AGAINST)) {
                    self.set_path(AGAINST, &path);
                }
            }
            ANALYZE => {
                if let Some(file) = &self.controller.selected {
                    let input = file.path.clone();
                    self.start(if self.kind == FileKind::Data {
                        Job::Analyze { input }
                    } else {
                        let against = self.path(AGAINST);
                        Job::Validate {
                            archive: input,
                            against: (!against.as_os_str().is_empty()).then_some(against),
                        }
                    });
                }
            }
            EXECUTE => {
                if let Some(file) = &self.controller.selected {
                    let input = file.path.clone();
                    let output = self.path(OUTPUT);
                    if output.as_os_str().is_empty() {
                        MessageBoxW(
                            self.handle,
                            wide("Choose a destination filename first.").as_ptr(),
                            wide("DataPack").as_ptr(),
                            MB_OK,
                        );
                        SetFocus(self.control(OUTPUT));
                        return;
                    }
                    self.start(if self.kind == FileKind::Data {
                        Job::Compress {
                            input,
                            output,
                            choice: if SendMessageW(self.control(METHOD), CB_GETCURSEL, 0, 0) == 1 {
                                CompressionChoice::Chunked
                            } else {
                                CompressionChoice::Recommended
                            },
                        }
                    } else {
                        Job::Decompress {
                            archive: input,
                            output,
                        }
                    });
                }
            }
            CANCEL => self.controller.cancel(),
            DETAILS => self.details = !self.details,
            OPEN_ARCHIVE => {
                if let State::CompressComplete(result) = &self.controller.state {
                    let path = result.output.clone();
                    if let Some(file) = &self.controller.selected {
                        self.set_path(AGAINST, &file.path);
                    }
                    self.kind = FileKind::Archive;
                    self.selected_path = None;
                    self.start(Job::Select {
                        path,
                        kind: FileKind::Archive,
                    });
                }
            }
            HELP => {
                MessageBoxW(self.handle, wide("Select data → Analyze → Compress → Validate → Decompress.\n\nUse Automatic for the engine recommendation or Chunked for bounded in-flight data. Estimates from a sample are not a guarantee.\n\nExact bytes preserves the original file, including line endings, whitespace and formatting. V1 needs an original-file comparison to verify byte integrity; V2 includes SHA-256 checks.\n\nCancel requests a safe stop. Keep the app open until the operation finishes. An already committed result wins over late cancellation.\n\nChoose a new filename if a destination already exists. Use Tab to navigate controls and Ctrl+C to copy selected paths or result text.").as_ptr(), wide("DataPack — Quick help").as_ptr(), MB_OK);
            }
            _ => {}
        }
        self.refresh();
        self.layout();
    }
}

// SAFETY: Windows invokes this callback on the owning thread. lpCreateParams
// points into the Box retained by run() until after the window/message loop ends.
unsafe extern "system" fn window_proc(
    handle: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        SetWindowLongPtrW(handle, GWLP_USERDATA, create.lpCreateParams as isize);
    }
    let pointer = GetWindowLongPtrW(handle, GWLP_USERDATA) as *const RefCell<Window>;
    if !pointer.is_null() {
        if let Ok(mut state) = (*pointer).try_borrow_mut() {
            match message {
                WM_COMMAND if (wparam >> 16) == BN_CLICKED as usize => {
                    state.command((wparam & 0xffff) as i32);
                    return 0;
                }
                WM_TIMER => {
                    state.refresh();
                    if state.smoke {
                        state.smoke_ticks += 1;
                        if state.smoke_ticks == 4 {
                            state.smoke_passed = state.controls.len() >= 24
                                && state.controls.iter().all(|c| IsWindow(c.handle) != 0)
                                && IsWindowVisible(handle) != 0
                                && !state.controller.busy();
                            PostMessageW(handle, WM_CLOSE, 0, 0);
                        }
                    }
                    return 0;
                }
                WM_SIZE => {
                    state.layout();
                    return 0;
                }
                WM_DPICHANGED => {
                    let rect = &*(lparam as *const RECT);
                    SetWindowPos(
                        handle,
                        null_mut(),
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    state.set_fonts();
                    state.layout();
                    return 0;
                }
                WM_GETMINMAXINFO => {
                    let limits = &mut *(lparam as *mut MINMAXINFO);
                    let dpi = GetDpiForWindow(handle).max(96) as i32;
                    limits.ptMinTrackSize = POINT {
                        x: 960 * dpi / 96,
                        y: 710 * dpi / 96,
                    };
                    return 0;
                }
                WM_CLOSE if state.controller.busy() => {
                    if MessageBoxW(handle, wide("An operation is running. Request safe cancellation and close when it finishes?\n\nChoose No to keep working.").as_ptr(), wide("DataPack").as_ptr(), MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2) == IDYES {
                        state.controller.cancel(); state.close_after_work = true;
                    }
                    return 0;
                }
                WM_QUERYENDSESSION if state.controller.busy() => {
                    state.controller.cancel();
                    return 0;
                }
                WM_CTLCOLORSTATIC => {
                    let dc = wparam as HDC;
                    SetBkColor(dc, GetSysColor(COLOR_WINDOW));
                    SetTextColor(dc, GetSysColor(COLOR_WINDOWTEXT));
                    return GetSysColorBrush(COLOR_WINDOW) as isize;
                }
                WM_DESTROY => {
                    KillTimer(handle, TIMER);
                    PostQuitMessage(0);
                    return 0;
                }
                _ => {}
            }
        }
    }
    DefWindowProcW(handle, message, wparam, lparam)
}

pub fn show_error(message: &str) {
    // SAFETY: null owner is allowed, UTF-16 buffers outlive the synchronous call.
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(message).as_ptr(),
            wide("DataPack Desktop").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn run(smoke: bool) -> Result<(), String> {
    // SAFETY: all window/control ownership is confined to this UI thread. Native
    // callbacks only access the live boxed RefCell; it is freed after WM_QUIT.
    unsafe {
        let common = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_PROGRESS_CLASS | ICC_STANDARD_CLASSES,
        };
        if InitCommonControlsEx(&common) == 0 {
            return Err("Windows controls could not be initialized.".into());
        }
        let instance = GetModuleHandleW(null());
        let class = wide("DataPackDesktopWindow");
        let mut spec: WNDCLASSW = std::mem::zeroed();
        spec.lpfnWndProc = Some(window_proc);
        spec.hInstance = instance;
        spec.lpszClassName = class.as_ptr();
        spec.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        spec.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
        spec.hbrBackground = (COLOR_WINDOW + 1) as HBRUSH;
        if RegisterClassW(&spec) == 0 {
            return Err("The DataPack window class could not be registered.".into());
        }
        let state = Box::new(RefCell::new(Window::new(smoke)));
        let dpi = GetDpiForSystem().max(96) as i32;
        let mut work_area: RECT = std::mem::zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work_area as *mut RECT).cast(), 0);
        let initial_width = (1040 * dpi / 96).min((work_area.right - work_area.left).max(960));
        let initial_height = (900 * dpi / 96).min((work_area.bottom - work_area.top).max(710));
        let handle = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.as_ptr(),
            wide("DataPack Desktop").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            initial_width,
            initial_height,
            null_mut(),
            null_mut(),
            instance,
            (&*state as *const RefCell<Window>).cast(),
        );
        if handle.is_null() {
            return Err("The DataPack window could not be created.".into());
        }
        {
            let mut ui = state.borrow_mut();
            ui.handle = handle;
            if let Err(error) = ui.create() {
                DestroyWindow(handle);
                return Err(error);
            }
            ui.layout();
        }
        ShowWindow(handle, SW_SHOW);
        UpdateWindow(handle);
        if SetTimer(handle, TIMER, 80, None) == 0 {
            DestroyWindow(handle);
            return Err("The Desktop event poll could not start.".into());
        }
        let mut message: MSG = std::mem::zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result == -1 {
                return Err("The Windows event loop failed.".into());
            }
            if result == 0 {
                break;
            }
            if IsDialogMessageW(handle, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let mut ui = state.borrow_mut();
        for font in ui.fonts.drain(..) {
            if !font.is_null() {
                DeleteObject(font);
            }
        }
        if smoke && !ui.smoke_passed {
            return Err("The native window/control smoke did not pass.".into());
        }
        Ok(())
    }
}
