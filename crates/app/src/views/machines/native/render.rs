use super::*;
use gpui::{canvas, MouseButton};

impl Render for Viewer {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let palette = theme::palette(cx);
    self.active = window.is_window_active();
    let viewport = self.viewport.clone();
    let input_error = self.input.error.borrow().clone();
    let running = matches!(self.state, Some(MachineState::Running));
    let paused = matches!(self.state, Some(MachineState::Paused));
    div()
      .flex()
      .flex_col()
      .size_full()
      .bg(palette.bg_surface)
      .child(
        div()
          .flex()
          .items_center()
          .justify_between()
          .gap_3()
          .p_3()
          .border_b_1()
          .border_color(palette.border_subtle)
          .child(
            div()
              .flex()
              .flex_col()
              .flex_1()
              .min_w_0()
              .gap_1()
              .child(
                div()
                  .truncate()
                  .child(Text::new(self.name.clone()).medium()),
              )
              .child(Text::new(self.label().to_owned()).size(Size::Xs).dimmed()),
          )
          .child(
            Group::new()
              .gap(Size::Xs)
              .child(
                Button::new("pause", if paused { "Resume" } else { "Pause" })
                  .size(Size::Sm)
                  .variant(Variant::Light)
                  .disabled(self.busy || (!running && !paused))
                  .on_click(cx.listener(move |this, _, _, cx| {
                    this.command(
                      if paused {
                        Command::Resume {}
                      } else {
                        Command::Pause {}
                      },
                      cx,
                    );
                  })),
              )
              .child(
                Button::new("stop", "Stop")
                  .size(Size::Sm)
                  .variant(Variant::Subtle)
                  .disabled(self.busy || (!running && !paused))
                  .on_click(cx.listener(|this, _, _, cx| this.command(Command::Stop {}, cx))),
              ),
          ),
      )
      .when_some(
        self
          .command_error
          .clone()
          .or_else(|| input_error.clone())
          .or_else(|| self.error.clone()),
        |view, error| {
          view.child(
            div().p_3().child(
              Text::new(error)
                .size(Size::Sm)
                .color(guise::theme::theme(cx).color(ColorName::Red, 6)),
            ),
          )
        },
      )
      .child(
        div()
          .flex()
          .flex_1()
          .min_h_0()
          .items_center()
          .justify_center()
          .overflow_hidden()
          .relative()
          .id("guest-display")
          .track_focus(&self.focus)
          .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
            this.key(&event.keystroke, if event.is_held { 2 } else { 1 });
            if this.enabled {
              cx.stop_propagation();
            }
          }))
          .on_key_up(cx.listener(|this, event: &gpui::KeyUpEvent, _, cx| {
            this.key(&event.keystroke, 0);
            if this.enabled {
              cx.stop_propagation();
            }
          }))
          .on_modifiers_changed(
            cx.listener(|this, event: &gpui::ModifiersChangedEvent, _, _| {
              this.modifiers(event.modifiers);
            }),
          )
          .on_mouse_move(
            cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
              this.pointer(event.position, None, window, cx);
            }),
          )
          .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
              this.pointer(event.position, Some((MouseButton::Left, true)), window, cx);
              this.modifiers(event.modifiers);
            }),
          )
          .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, event: &gpui::MouseUpEvent, window, cx| {
              this.pointer(event.position, Some((MouseButton::Left, false)), window, cx);
            }),
          )
          .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
              this.pointer(event.position, Some((MouseButton::Right, true)), window, cx);
            }),
          )
          .on_mouse_up(
            MouseButton::Right,
            cx.listener(|this, event: &gpui::MouseUpEvent, window, cx| {
              this.pointer(
                event.position,
                Some((MouseButton::Right, false)),
                window,
                cx,
              );
            }),
          )
          .on_mouse_down(
            MouseButton::Middle,
            cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
              this.pointer(
                event.position,
                Some((MouseButton::Middle, true)),
                window,
                cx,
              );
            }),
          )
          .on_mouse_up(
            MouseButton::Middle,
            cx.listener(|this, event: &gpui::MouseUpEvent, window, cx| {
              this.pointer(
                event.position,
                Some((MouseButton::Middle, false)),
                window,
                cx,
              );
            }),
          )
          .on_mouse_up_out(
            MouseButton::Middle,
            cx.listener(|this, _, _, _| this.release()),
          )
          .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.release()),
          )
          .on_mouse_up_out(
            MouseButton::Right,
            cx.listener(|this, _, _, _| this.release()),
          )
          .child(
            canvas(move |bounds, _, _| viewport.set(bounds), |_, _, _, _| {})
              .absolute()
              .size_full(),
          )
          .children(self.image.clone().map(|image| img(image).size_full()))
          .when(self.image.is_none(), |view| {
            view.child(Text::new(self.label().to_owned()).dimmed())
          }),
      )
      .child(
        div()
          .p_2()
          .border_t_1()
          .border_color(palette.border_subtle)
          .child(
            Text::new(
              "Click the display to control the guest · Closing this window keeps the VM running",
            )
            .size(Size::Xs)
            .dimmed(),
          ),
      )
  }
}
