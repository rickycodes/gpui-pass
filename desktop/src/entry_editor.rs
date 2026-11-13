use crate::PassApp;
use gpui::{
    App, Bounds, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    InspectorElementId, LayoutId, PaintQuad, Pixels, Point, ShapedLine, Style, TextRun, Window,
    fill, point, prelude::*, px, relative, rgba, size,
};
use std::ops::Range;

#[derive(Clone)]
pub(crate) struct LineLayout {
    pub range: Range<usize>,
    pub bounds: Bounds<Pixels>,
    pub shaped: ShapedLine,
}

#[derive(Clone)]
pub(crate) struct TextLayout {
    pub lines: Vec<LineLayout>,
}

impl TextLayout {
    pub fn index_for_point(&self, point: Point<Pixels>) -> usize {
        let Some(line) = self.lines.iter().find(|line| {
            point.y >= line.bounds.top() && point.y <= line.bounds.bottom()
        }).or_else(|| {
            if point.y < self.lines.first()?.bounds.top() {
                self.lines.first()
            } else {
                self.lines.last()
            }
        }) else { return 0; };
        if point.x <= line.bounds.left() {
            return line.range.start;
        }
        if point.x >= line.bounds.right() {
            return line.range.end;
        }
        line.shaped
            .index_for_x(point.x - line.bounds.left())
            .map_or(line.range.end, |index| line.range.start + index)
    }
}

pub(crate) struct EntryTextElement {
    pub app: Entity<PassApp>,
}

struct PreparedLine {
    line: ShapedLine,
    bounds: Bounds<Pixels>,
    selection: Option<PaintQuad>,
    cursor: Option<PaintQuad>,
}

pub struct PrepaintState {
    lines: Vec<PreparedLine>,
    layout: TextLayout,
}

impl IntoElement for EntryTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EntryTextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> { None }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> { None }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let rows = self.app.read(cx).generator_name.matches('\n').count() + 1;
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (window.line_height() * rows as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let app = self.app.read(cx);
        let content = app.generator_name.as_str();
        let selected = app.generator_name_selection.clone();
        let cursor_offset = app.generator_name_cursor;
        let focused = app.generator_name_focus.is_focused(window);
        let text_style = window.text_style();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let row_height = window.line_height();
        let mut byte_start = 0;
        let mut prepared = Vec::new();
        let mut layouts = Vec::new();

        for (row, text) in content.split('\n').enumerate() {
            let range = byte_start..byte_start + text.len();
            let text_run = TextRun {
                len: text.len(),
                font: text_style.font(),
                color: text_style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window.text_system().shape_line(text.to_owned().into(), font_size, &[text_run], None);
            let line_bounds = Bounds::new(
                point(bounds.left(), bounds.top() + row_height * row as f32),
                size(bounds.size.width, row_height),
            );
            let overlap_start = selected.start.max(range.start).min(range.end);
            let overlap_end = selected.end.max(range.start).min(range.end);
            let selection = (overlap_start < overlap_end).then(|| {
                fill(
                    Bounds::from_corners(
                        point(line_bounds.left() + shaped.x_for_index(overlap_start - range.start), line_bounds.top()),
                        point(line_bounds.left() + shaped.x_for_index(overlap_end - range.start), line_bounds.bottom()),
                    ),
                    rgba(app.theme.colors.selection),
                )
            });
            let cursor = (focused && selected.is_empty() && cursor_offset >= range.start && cursor_offset <= range.end)
                .then(|| fill(
                    Bounds::new(
                        point(line_bounds.left() + shaped.x_for_index(cursor_offset - range.start), line_bounds.top()),
                        size(px(1.5), line_bounds.size.height),
                    ),
                    gpui::rgb(app.theme.colors.accent),
                ));
            layouts.push(LineLayout { range, bounds: line_bounds, shaped: shaped.clone() });
            prepared.push(PreparedLine { line: shaped, bounds: line_bounds, selection, cursor });
            byte_start += text.len() + 1;
        }

        PrepaintState { lines: prepared, layout: TextLayout { lines: layouts } }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.app.read(cx).generator_name_focus.clone();
        window.handle_input(&focus, ElementInputHandler::new(
            prepaint.layout.lines.first().map_or(Bounds::default(), |line| {
                let last = prepaint.layout.lines.last().unwrap();
                Bounds::from_corners(line.bounds.origin, point(last.bounds.right(), last.bounds.bottom()))
            }),
            self.app.clone(),
        ), cx);
        for prepared in &mut prepaint.lines {
            if let Some(selection) = prepared.selection.take() { window.paint_quad(selection); }
            prepared.line.paint(prepared.bounds.origin, prepared.bounds.size.height, window, cx).ok();
            if let Some(cursor) = prepared.cursor.take() { window.paint_quad(cursor); }
        }
        self.app.update(cx, |app, _| app.editor_layout = Some(prepaint.layout.clone()));
    }
}
