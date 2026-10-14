//! Prototipo mínimo P4.0: valida Iced 0.14 (compila + renderiza en Linux).
//!
//! Estado (opciones), Message (cambios), update (procesa) y view
//! (renderiza con `row!`/`column!`). Sin async: no se necesita Tokio.

use iced::widget::{button, column, pick_list, row, text, toggler};
use iced::{Element, Task};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overwrite {
    Never,
    IfNewer,
    IfDifferent,
    Always,
}

impl std::fmt::Display for Overwrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Overwrite::Never => "Never",
            Overwrite::IfNewer => "IfNewer",
            Overwrite::IfDifferent => "IfDifferent",
            Overwrite::Always => "Always",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone)]
enum Message {
    PreserveToggled(bool),
    OverwriteSelected(Overwrite),
    Simulate,
}

struct State {
    preserve: bool,
    overwrite: Overwrite,
    status: String,
}

fn boot() -> (State, Task<Message>) {
    (
        State {
            preserve: true,
            overwrite: Overwrite::Always,
            status: String::from("Listo. Ajusta las opciones y pulsa «Simular copia»."),
        },
        Task::none(),
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::PreserveToggled(on) => {
            state.preserve = on;
        }
        Message::OverwriteSelected(o) => {
            state.overwrite = o;
        }
        Message::Simulate => {
            state.status = format!(
                "Simulación: preserve_metadata={}, overwrite={}",
                state.preserve, state.overwrite
            );
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    column![
        text("CopyForge Proto").size(28),
        toggler(state.preserve)
            .label("Preserve metadata")
            .on_toggle(Message::PreserveToggled),
        row![
            text("Overwrite:"),
            pick_list(
                &OPTIONS[..],
                Some(state.overwrite),
                Message::OverwriteSelected
            ),
        ]
        .spacing(10),
        button("Simular copia").on_press(Message::Simulate),
        text(&state.status),
    ]
    .spacing(15)
    .padding(20)
    .into()
}

const OPTIONS: [Overwrite; 4] = [
    Overwrite::Never,
    Overwrite::IfNewer,
    Overwrite::IfDifferent,
    Overwrite::Always,
];

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("CopyForge Proto")
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_and_select_update_state() {
        let (mut state, _) = boot();
        assert!(state.preserve);
        let _ = update(&mut state, Message::PreserveToggled(false));
        assert!(!state.preserve);
        let _ = update(&mut state, Message::OverwriteSelected(Overwrite::Never));
        assert_eq!(state.overwrite, Overwrite::Never);
    }

    #[test]
    fn simulate_reflects_options_in_status() {
        let (mut state, _) = boot();
        state.preserve = false;
        state.overwrite = Overwrite::IfDifferent;
        let _ = update(&mut state, Message::Simulate);
        assert!(
            state.status.contains("preserve_metadata=false"),
            "status: {}",
            state.status
        );
        assert!(
            state.status.contains("IfDifferent"),
            "status: {}",
            state.status
        );
    }
}
