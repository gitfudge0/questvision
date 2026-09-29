//! Small ASCII settings field using GPUI's native text/IME interface.
//! The only editable values are IP addresses and unsigned integers.
use gpui::{prelude::*, *};
use std::ops::Range;

actions!(
    settings_input,
    [
        Backspace, Delete, Left, Right, SelectAll, Home, End, Paste, Copy, Cut
    ]
);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("SettingsInput")),
        KeyBinding::new("delete", Delete, Some("SettingsInput")),
        KeyBinding::new("left", Left, Some("SettingsInput")),
        KeyBinding::new("right", Right, Some("SettingsInput")),
        KeyBinding::new("home", Home, Some("SettingsInput")),
        KeyBinding::new("end", End, Some("SettingsInput")),
        KeyBinding::new("secondary-a", SelectAll, Some("SettingsInput")),
        KeyBinding::new("secondary-v", Paste, Some("SettingsInput")),
        KeyBinding::new("secondary-c", Copy, Some("SettingsInput")),
        KeyBinding::new("secondary-x", Cut, Some("SettingsInput")),
    ]);
}

pub(super) struct Input {
    focus: FocusHandle,
    pub value: String,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    layout: Option<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
}

impl Input {
    pub fn new(value: String, cx: &mut Context<Self>) -> Self {
        let end = value.len();
        Self {
            focus: cx.focus_handle(),
            value,
            selection: end..end,
            marked: None,
            layout: None,
            bounds: None,
            scroll_x: px(0.),
        }
    }
    fn move_cursor(&mut self, pos: usize, cx: &mut Context<Self>) {
        self.selection = pos..pos;
        self.marked = None;
        cx.notify();
    }
    fn replace(&mut self, range: Option<Range<usize>>, text: &str, cx: &mut Context<Self>) {
        // IP addresses and numeric settings have no non-ASCII representation.
        let text: String = text
            .chars()
            .filter(|ch| ch.is_ascii_graphic())
            .take(64)
            .collect();
        let range = range
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection.clone());
        let start = range.start.min(self.value.len());
        let end = range.end.max(start).min(self.value.len());
        if self.value.len() - (end - start) + text.len() > 64 {
            return;
        }
        self.value.replace_range(start..end, &text);
        self.marked = None;
        self.move_cursor(start + text.len(), cx);
    }
    fn copy(&self, cx: &mut Context<Self>) {
        if !self.selection.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.value[self.selection.clone()].into(),
            ));
        }
    }
}

impl EntityInputHandler for Input {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = range.start.min(self.value.len())..range.end.min(self.value.len());
        *actual = Some(range.clone());
        self.value.get(range).map(str::to_owned)
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.selection.clone(),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone()
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace(range, text, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = range
            .as_ref()
            .or(self.marked.as_ref())
            .unwrap_or(&self.selection)
            .start
            .min(self.value.len());
        self.replace(range, text, cx);
        let end = self.selection.end;
        if end > start {
            self.marked = Some(start..end);
        }
        if let Some(selected) = selected {
            self.selection = (start + selected.start).min(end)..(start + selected.end).min(end);
        }
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.layout.as_ref()?;
        let bounds = self.bounds.unwrap_or(bounds);
        Some(Bounds::from_corners(
            point(
                bounds.left() + line.x_for_index(range.start.min(self.value.len())),
                bounds.top(),
            ),
            point(
                bounds.left() + line.x_for_index(range.end.min(self.value.len())),
                bounds.bottom(),
            ),
        ))
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(
            self.layout
                .as_ref()?
                .closest_index_for_x(point.x - self.bounds?.left())
                .min(self.value.len()),
        )
    }
}
impl Focusable for Input {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Input {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let input = cx.entity();
        let paint_input = input.clone();
        div()
            .w_full()
            .border_1()
            .rounded_md()
            .border_color(rgb(if self.focus.is_focused(window) {
                0x56cdb4
            } else {
                0x35414a
            }))
            .bg(rgb(0x12191e))
            .px_3()
            .py_2()
            .overflow_hidden()
            .key_context("SettingsInput")
            .track_focus(&self.focus)
            .tab_stop(true)
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window);
                    let pos = this
                        .layout
                        .as_ref()
                        .zip(this.bounds)
                        .map(|(line, bounds)| {
                            line.closest_index_for_x(event.position.x - bounds.left())
                                .min(this.value.len())
                        })
                        .unwrap_or(this.value.len());
                    this.move_cursor(pos, cx);
                }),
            )
            .on_action(cx.listener(|this, _: &Left, _, cx| {
                this.move_cursor(
                    if this.selection.is_empty() {
                        this.selection.start.saturating_sub(1)
                    } else {
                        this.selection.start
                    },
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &Right, _, cx| {
                this.move_cursor(
                    if this.selection.is_empty() {
                        (this.selection.end + 1).min(this.value.len())
                    } else {
                        this.selection.end
                    },
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &Home, _, cx| this.move_cursor(0, cx)))
            .on_action(cx.listener(|this, _: &End, _, cx| this.move_cursor(this.value.len(), cx)))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.selection = 0..this.value.len();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Backspace, _, cx| {
                if this.selection.is_empty() {
                    this.selection.start = this.selection.start.saturating_sub(1);
                }
                this.replace(None, "", cx);
            }))
            .on_action(cx.listener(|this, _: &Delete, _, cx| {
                if this.selection.is_empty() {
                    this.selection.end = (this.selection.end + 1).min(this.value.len());
                }
                this.replace(None, "", cx);
            }))
            .on_action(cx.listener(|this, _: &Paste, _, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    this.replace(None, &text, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Copy, _, cx| this.copy(cx)))
            .on_action(cx.listener(|this, _: &Cut, _, cx| {
                this.copy(cx);
                this.replace(None, "", cx);
            }))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let style = window.text_style();
                        let value: SharedString = input.read(cx).value.clone().into();
                        let run = TextRun {
                            len: value.len(),
                            font: style.font(),
                            color: style.color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line = window
                            .text_system()
                            .shape_line(value, px(14.), &[run], None);
                        input.update(cx, |input, _| {
                            let cursor_x = line.x_for_index(input.selection.end);
                            input.scroll_x = input
                                .scroll_x
                                .min(cursor_x)
                                .max((cursor_x - bounds.size.width + px(2.)).max(px(0.)));
                            input.layout = Some(line.clone());
                            input.bounds = Some(Bounds::new(
                                point(bounds.left() - input.scroll_x, bounds.top()),
                                bounds.size,
                            ));
                        });
                        line
                    },
                    move |bounds, line, window, cx| {
                        let input = paint_input.read(cx);
                        let focus = input.focus.clone();
                        let selection = input.selection.clone();
                        let text_bounds = input.bounds.unwrap_or(bounds);
                        window.handle_input(
                            &focus,
                            ElementInputHandler::new(bounds, paint_input.clone()),
                            cx,
                        );
                        if focus.is_focused(window) {
                            if !selection.is_empty() {
                                window.paint_quad(fill(
                                    Bounds::from_corners(
                                        point(
                                            text_bounds.left() + line.x_for_index(selection.start),
                                            bounds.top(),
                                        ),
                                        point(
                                            text_bounds.left() + line.x_for_index(selection.end),
                                            bounds.bottom(),
                                        ),
                                    ),
                                    rgba(0x56cdb440),
                                ));
                            } else {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(
                                            text_bounds.left() + line.x_for_index(selection.end),
                                            bounds.top(),
                                        ),
                                        size(px(1.), bounds.size.height),
                                    ),
                                    rgb(0x56cdb4),
                                ));
                            }
                        }
                        let _ = line.paint(text_bounds.origin, px(20.), window, cx);
                    },
                )
                .w_full()
                .h(px(20.)),
            )
    }
}
