//! P4.1: GUI Iced — `State` migrado desde `GuiOptions`.
//!
//! `apps/gui` (egui) no exponía ningún struct de opciones (es binario y usa
//! `CopyOptions::default()` con overrides sueltos), así que `GuiOptions` se
//! define aquí como nuevo. Candidato P4.2: unificar con egui o extraerlo a
//! un crate compartido.
//!
//! Los enums espejo (`OverwriteOpt`…) existen porque `pick_list` exige
//! `ToString` y no se puede implementar un trait foráneo sobre tipos de
//! `copyforge-core` (el motor no se toca en fase P4). Precedente: los
//! `*Arg` de `apps/cli/src/args.rs`.

use std::path::PathBuf;

use copyforge_core::{DurabilityPolicy, OverwritePolicy, VerifyPolicy};
use iced::widget::{button, column, pick_list, row, slider, text, text_input, toggler};
use iced::{Element, Task};

/// Opciones editables de la GUI (nuevo en P4.1).
#[derive(Debug, Clone)]
pub struct GuiOptions {
    pub preserve_metadata: bool,
    pub overwrite: OverwritePolicy,
    pub verify: VerifyPolicy,
    pub jobs: usize,
    pub exclude: String,
    pub durability: DurabilityPolicy,
}

impl Default for GuiOptions {
    fn default() -> Self {
        Self {
            preserve_metadata: true,
            overwrite: OverwritePolicy::Always,
            verify: VerifyPolicy::Size,
            jobs: 4,
            exclude: String::new(),
            durability: DurabilityPolicy::File,
        }
    }
}

macro_rules! mirror_enum {
    ($name:ident, $core:ty, [$($v:ident),+]) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum $name {
            $($v),+
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(match self {
                    $(Self::$v => stringify!($v)),+
                })
            }
        }
    };
}

mirror_enum!(OverwriteOpt, OverwritePolicy, [Never, IfNewer, IfDifferent, Always]);
mirror_enum!(VerifyOpt, VerifyPolicy, [None, Size, Hash]);
mirror_enum!(DurabilityOpt, DurabilityPolicy, [None, File, Full]);

impl From<OverwriteOpt> for OverwritePolicy {
    fn from(o: OverwriteOpt) -> Self {
        match o {
            OverwriteOpt::Never => OverwritePolicy::Never,
            OverwriteOpt::IfNewer => OverwritePolicy::IfNewer,
            OverwriteOpt::IfDifferent => OverwritePolicy::IfDifferent,
            OverwriteOpt::Always => OverwritePolicy::Always,
        }
    }
}

impl From<OverwritePolicy> for OverwriteOpt {
    fn from(o: OverwritePolicy) -> Self {
        match o {
            OverwritePolicy::Never => OverwriteOpt::Never,
            OverwritePolicy::IfNewer => OverwriteOpt::IfNewer,
            OverwritePolicy::IfDifferent => OverwriteOpt::IfDifferent,
            OverwritePolicy::Always => OverwriteOpt::Always,
        }
    }
}

impl From<VerifyOpt> for VerifyPolicy {
    fn from(v: VerifyOpt) -> Self {
        match v {
            VerifyOpt::None => VerifyPolicy::None,
            VerifyOpt::Size => VerifyPolicy::Size,
            VerifyOpt::Hash => VerifyPolicy::Hash,
        }
    }
}

impl From<VerifyPolicy> for VerifyOpt {
    fn from(v: VerifyPolicy) -> Self {
        match v {
            VerifyPolicy::None => VerifyOpt::None,
            VerifyPolicy::Size => VerifyOpt::Size,
            VerifyPolicy::Hash => VerifyOpt::Hash,
        }
    }
}

impl From<DurabilityOpt> for DurabilityPolicy {
    fn from(d: DurabilityOpt) -> Self {
        match d {
            DurabilityOpt::None => DurabilityPolicy::None,
            DurabilityOpt::File => DurabilityPolicy::File,
            DurabilityOpt::Full => DurabilityPolicy::Full,
        }
    }
}

impl From<DurabilityPolicy> for DurabilityOpt {
    fn from(d: DurabilityPolicy) -> Self {
        match d {
            DurabilityPolicy::None => DurabilityOpt::None,
            DurabilityPolicy::File => DurabilityOpt::File,
            DurabilityPolicy::Full => DurabilityOpt::Full,
        }
    }
}

const OVERWRITES: [OverwriteOpt; 4] = [
    OverwriteOpt::Never,
    OverwriteOpt::IfNewer,
    OverwriteOpt::IfDifferent,
    OverwriteOpt::Always,
];
const VERIFIES: [VerifyOpt; 3] = [VerifyOpt::None, VerifyOpt::Size, VerifyOpt::Hash];
const DURABILITIES: [DurabilityOpt; 3] =
    [DurabilityOpt::None, DurabilityOpt::File, DurabilityOpt::Full];

struct State {
    options: GuiOptions,
    src: PathBuf,
    dst: PathBuf,
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    TogglePreserveMetadata(bool),
    ChangeOverwrite(OverwritePolicy),
    ChangeVerify(VerifyPolicy),
    ChangeJobs(usize),
    ChangeExclude(String),
    ChangeDurability(DurabilityPolicy),
    ChangeSrc(String),
    ChangeDst(String),
    Simulate,
}

fn boot() -> (State, Task<Message>) {
    (
        State {
            options: GuiOptions::default(),
            src: PathBuf::new(),
            dst: PathBuf::new(),
            status: String::from("Listo. Ajusta las opciones y pulsa «Simular copia»."),
        },
        Task::none(),
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::TogglePreserveMetadata(on) => {
            state.options.preserve_metadata = on;
        }
        Message::ChangeOverwrite(o) => {
            state.options.overwrite = o;
        }
        Message::ChangeVerify(v) => {
            state.options.verify = v;
        }
        Message::ChangeJobs(j) => {
            state.options.jobs = j.clamp(1, 16);
        }
        Message::ChangeExclude(x) => {
            state.options.exclude = x;
        }
        Message::ChangeDurability(d) => {
            state.options.durability = d;
        }
        Message::ChangeSrc(s) => {
            state.src = PathBuf::from(s);
        }
        Message::ChangeDst(s) => {
            state.dst = PathBuf::from(s);
        }
        Message::Simulate => {
            state.status = format!(
                "Simulación: src={} dst={} preserve={} overwrite={:?} verify={:?} jobs={} exclude={} durability={:?}",
                state.src.display(),
                state.dst.display(),
                state.options.preserve_metadata,
                state.options.overwrite,
                state.options.verify,
                state.options.jobs,
                state.options.exclude,
                state.options.durability,
            );
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    column![
        text("CopyForge (Iced)").size(28),
        row![
            text("Origen:"),
            text_input("Ruta origen…", &state.src.to_string_lossy())
                .on_input(Message::ChangeSrc),
        ]
        .spacing(10),
        row![
            text("Destino:"),
            text_input("Ruta destino…", &state.dst.to_string_lossy())
                .on_input(Message::ChangeDst),
        ]
        .spacing(10),
        toggler(state.options.preserve_metadata)
            .label("Preserve metadata")
            .on_toggle(Message::TogglePreserveMetadata),
        row![
            text("Overwrite:"),
            pick_list(
                &OVERWRITES[..],
                Some(OverwriteOpt::from(state.options.overwrite)),
                |o| Message::ChangeOverwrite(o.into()),
            ),
        ]
        .spacing(10),
        row![
            text("Verify:"),
            pick_list(
                &VERIFIES[..],
                Some(VerifyOpt::from(state.options.verify)),
                |v| Message::ChangeVerify(v.into()),
            ),
        ]
        .spacing(10),
        row![
            text("Jobs:"),
            // slider trabaja en u32 (usize no implementa Into<f64>);
            // el rango acotado hace los casts exactos.
            slider(1..=16u32, state.options.jobs as u32, |v| {
                Message::ChangeJobs(v as usize)
            }),
            text(state.options.jobs.to_string()),
        ]
        .spacing(10),
        row![
            text("Exclude:"),
            text_input("*.log, target/**…", &state.options.exclude)
                .on_input(Message::ChangeExclude),
        ]
        .spacing(10),
        row![
            text("Durability:"),
            pick_list(
                &DURABILITIES[..],
                Some(DurabilityOpt::from(state.options.durability)),
                |d| Message::ChangeDurability(d.into()),
            ),
        ]
        .spacing(10),
        button("Simular copia").on_press(Message::Simulate),
        text(&state.status),
    ]
    .spacing(12)
    .padding(20)
    .into()
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("CopyForge (Iced)")
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_state() -> State {
        boot().0
    }

    #[test]
    fn toggle_updates_preserve() {
        let mut s = fresh_state();
        assert!(s.options.preserve_metadata);
        let _ = update(&mut s, Message::TogglePreserveMetadata(false));
        assert!(!s.options.preserve_metadata);
    }

    #[test]
    fn change_overwrite_updates_policy() {
        let mut s = fresh_state();
        let _ = update(&mut s, Message::ChangeOverwrite(OverwritePolicy::Never));
        assert_eq!(s.options.overwrite, OverwritePolicy::Never);
    }

    #[test]
    fn change_verify_updates_policy() {
        let mut s = fresh_state();
        let _ = update(&mut s, Message::ChangeVerify(VerifyPolicy::Hash));
        assert_eq!(s.options.verify, VerifyPolicy::Hash);
    }

    #[test]
    fn change_jobs_updates_and_clamps() {
        let mut s = fresh_state();
        let _ = update(&mut s, Message::ChangeJobs(8));
        assert_eq!(s.options.jobs, 8);
        let _ = update(&mut s, Message::ChangeJobs(0));
        assert_eq!(s.options.jobs, 1);
    }

    #[test]
    fn change_exclude_and_durability_update() {
        let mut s = fresh_state();
        let _ = update(&mut s, Message::ChangeExclude(String::from("*.log")));
        assert_eq!(s.options.exclude, "*.log");
        let _ = update(&mut s, Message::ChangeDurability(DurabilityPolicy::Full));
        assert_eq!(s.options.durability, DurabilityPolicy::Full);
    }

    #[test]
    fn simulate_reflects_state_in_status() {
        let mut s = fresh_state();
        let _ = update(&mut s, Message::ChangeSrc(String::from("/a")));
        let _ = update(&mut s, Message::ChangeDst(String::from("/b")));
        let _ = update(&mut s, Message::Simulate);
        assert!(s.status.contains("/a"), "status: {}", s.status);
        assert!(s.status.contains("jobs=4"), "status: {}", s.status);
    }
}
