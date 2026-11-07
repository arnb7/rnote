// Imports
use super::PenBehaviour;
use super::PenStyle;
use super::pensconfig::brushconfig::BrushStyle;
use crate::engine::{EngineView, EngineViewMut};
use crate::store::StrokeKey;
use crate::strokes::BrushStroke;
use crate::strokes::Stroke;
use crate::{DrawableOnDoc, WidgetFlags};
use p2d::bounding_volume::{Aabb, BoundingVolume};
use piet::RenderContext;
use rnote_compose::Constraints;
use rnote_compose::Style;
use rnote_compose::builders::buildable::{Buildable, BuilderCreator, BuilderProgress};
use rnote_compose::builders::{
    PenPathBuilderType, PenPathCurvedBuilder, PenPathModeledBuilder, PenPathSimpleBuilder,
};
use rnote_compose::eventresult::{EventPropagation, EventResult};
use rnote_compose::penevent::{PenEvent, PenProgress};
use rnote_compose::penpath::{Element, Segment};
use rnote_compose::shapes::Shapeable;
use std::time::Instant;

#[derive(Debug)]
enum BrushState {
    Idle,
    Drawing {
        path_builder: Box<dyn Buildable<Emit = Segment>>,
        current_stroke_key: StrokeKey,
        preview_style: Style,
        start_time: Instant,
    },
}

#[derive(Debug)]
pub struct Brush {
    state: BrushState,
    hover_element: Option<Element>,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            state: BrushState::Idle,
            hover_element: None,
        }
    }
}

impl PenBehaviour for Brush {
    fn init(&mut self, _engine_view: &EngineView) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn deinit(&mut self) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn style(&self) -> PenStyle {
        PenStyle::Brush
    }

    fn update_state(&mut self, _engine_view: &mut EngineViewMut) -> WidgetFlags {
        WidgetFlags::default()
    }

    fn handle_event(
        &mut self,
        event: PenEvent,
        now: Instant,
        engine_view: &mut EngineViewMut,
    ) -> (EventResult<PenProgress>, WidgetFlags) {
        let mut widget_flags = WidgetFlags::default();

        let event_result = match (&mut self.state, event) {
            (BrushState::Idle, PenEvent::Down { element, .. }) => {
                if !element.filter_by_bounds(
                    engine_view
                        .document
                        .bounds()
                        .loosened(Self::INPUT_OVERSHOOT),
                ) {
                    #[cfg(feature = "ui")]
                    {
                        if engine_view.config.pens_config.brush_config.style == BrushStyle::Marker {
                            play_marker_sound(engine_view);
                        } else {
                            trigger_brush_sound(engine_view);
                        }
                    }

                    engine_view
                        .config
                        .pens_config
                        .brush_config
                        .new_style_seeds();

                    let preview_style = Self::get_preview_style(&engine_view.as_im());
                    let brushstroke =
                        Stroke::BrushStroke(BrushStroke::new(element, preview_style.clone()));

                    let current_stroke_key = engine_view.store.insert_stroke(
                        brushstroke,
                        Some(
                            engine_view
                                .config
                                .pens_config
                                .brush_config
                                .layer_for_current_options(),
                        ),
                    );

                    engine_view.store.regenerate_rendering_for_stroke(
                        current_stroke_key,
                        engine_view.camera.viewport(),
                        engine_view.camera.image_scale(),
                    );

                    self.state = BrushState::Drawing {
                        path_builder: new_builder(
                            engine_view.config.pens_config.brush_config.builder_type,
                            element,
                            now,
                        ),
                        current_stroke_key,
                        preview_style,
                        start_time: now,
                    };

                    EventResult {
                        handled: true,
                        propagate: EventPropagation::Stop,
                        progress: PenProgress::InProgress,
                    }
                } else {
                    self.hover_element = None;
                    EventResult {
                        handled: false,
                        propagate: EventPropagation::Proceed,
                        progress: PenProgress::Idle,
                    }
                }
            }
            (BrushState::Idle, PenEvent::Proximity { element, .. }) => {
                self.hover_element = Some(element);
                EventResult {
                    handled: false,
                    propagate: EventPropagation::Proceed,
                    progress: PenProgress::Idle,
                }
            }
            (BrushState::Idle, _) => {
                self.hover_element = None;
                EventResult {
                    handled: false,
                    propagate: EventPropagation::Proceed,
                    progress: PenProgress::Idle,
                }
            }
            (
                BrushState::Drawing {
                    current_stroke_key, ..
                },
                PenEvent::Cancel,
            ) => {
                if let Some(Stroke::BrushStroke(brushstroke)) =
                    engine_view.store.get_stroke_mut(*current_stroke_key)
                {
                    brushstroke.style = engine_view
                        .config
                        .pens_config
                        .brush_config
                        .style_for_current_options();
                }

                // Finish up the last stroke
                engine_view
                    .store
                    .update_geometry_for_stroke(*current_stroke_key);
                engine_view.store.regenerate_rendering_for_stroke_threaded(
                    engine_view.tasks_tx.clone(),
                    *current_stroke_key,
                    engine_view.camera.viewport(),
                    engine_view.camera.image_scale(),
                );
                widget_flags |= engine_view
                    .document
                    .resize_autoexpand(engine_view.store, engine_view.camera);

                self.state = BrushState::Idle;

                widget_flags |= engine_view.store.record(Instant::now());
                widget_flags.store_modified = true;

                EventResult {
                    handled: true,
                    propagate: EventPropagation::Stop,
                    progress: PenProgress::Finished,
                }
            }
            (
                BrushState::Drawing {
                    path_builder,
                    current_stroke_key,
                    start_time,
                    ..
                },
                pen_event,
            ) => {
                let builder_result =
                    path_builder.handle_event(pen_event, now, Constraints::default());
                let handled = builder_result.handled;
                let propagate = builder_result.propagate;

                let progress = match builder_result.progress {
                    BuilderProgress::InProgress => {
                        #[cfg(feature = "ui")]
                        {
                            if engine_view.config.pens_config.brush_config.style
                                != BrushStyle::Marker
                            {
                                trigger_brush_sound(engine_view);
                            }
                        }

                        PenProgress::InProgress
                    }
                    BuilderProgress::EmitContinue(segments) => {
                        #[cfg(feature = "ui")]
                        {
                            if engine_view.config.pens_config.brush_config.style
                                != BrushStyle::Marker
                            {
                                trigger_brush_sound(engine_view);
                            }
                        }

                        let n_segments = segments.len();

                        if n_segments != 0 {
                            if let Some(Stroke::BrushStroke(brushstroke)) =
                                engine_view.store.get_stroke_mut(*current_stroke_key)
                            {
                                brushstroke.extend_w_segments(segments);
                                widget_flags.store_modified = true;
                            }

                            engine_view.store.append_rendering_last_segments(
                                engine_view.tasks_tx.clone(),
                                *current_stroke_key,
                                n_segments,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                        }

                        PenProgress::InProgress
                    }
                    BuilderProgress::Finished(segments) => {
                        let n_segments = segments.len();

                        if n_segments != 0 {
                            if let Some(Stroke::BrushStroke(brushstroke)) =
                                engine_view.store.get_stroke_mut(*current_stroke_key)
                            {
                                brushstroke.extend_w_segments(segments);
                                widget_flags.store_modified = true;
                            }

                            engine_view.store.append_rendering_last_segments(
                                engine_view.tasks_tx.clone(),
                                *current_stroke_key,
                                n_segments,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                        }

                        // Check for Scribble Erase (G1)
                        let scribble_config = engine_view
                            .config
                            .pens_config
                            .brush_config
                            .scribble_config
                            .clone();

                        let mut stroke_points = Vec::new();
                        if let Some(Stroke::BrushStroke(brushstroke)) =
                            engine_view.store.get_stroke_ref(*current_stroke_key)
                        {
                            stroke_points.push(brushstroke.path.start.pos);
                            for seg in &brushstroke.path.segments {
                                match seg {
                                    Segment::LineTo { end } => stroke_points.push(end.pos),
                                    Segment::QuadBezTo { cp, end } => {
                                        stroke_points.push(*cp);
                                        stroke_points.push(end.pos);
                                    }
                                    Segment::CubBezTo { cp1, cp2, end } => {
                                        stroke_points.push(*cp1);
                                        stroke_points.push(*cp2);
                                        stroke_points.push(end.pos);
                                    }
                                }
                            }
                        }

                        let duration_secs = (now - *start_time).as_secs_f64();
                        let scribble_analysis =
                            if scribble_config.enable && stroke_points.len() >= 4 {
                                crate::pens::ScribbleDetector::analyze(
                                    &stroke_points,
                                    duration_secs,
                                    &scribble_config,
                                )
                            } else {
                                crate::pens::ScribbleAnalysis::default()
                            };

                        if scribble_analysis.is_scribble {
                            let scribble_bounds = scribble_analysis.bounds;
                            let candidate_keys: Vec<StrokeKey> = engine_view
                                .store
                                .stroke_keys_as_rendered_intersecting_bounds(scribble_bounds.loosened(12.0))
                                .into_iter()
                                .filter(|&k| k != *current_stroke_key)
                                .collect();

                            let mut candidate_hitboxes = Vec::new();
                            let mut candidate_points = Vec::new();
                            let mut directly_colliding_keys = Vec::new();

                            for &k in &candidate_keys {
                                if let Some(s) = engine_view.store.get_stroke_ref(k) {
                                    let hitboxes = s.hitboxes();
                                    let collides = hitboxes.is_empty()
                                        || hitboxes
                                            .iter()
                                            .any(|hb| scribble_bounds.intersects(&hb.loosened(12.0)));
                                    if collides {
                                        directly_colliding_keys.push(k);
                                    }
                                    candidate_hitboxes.extend(hitboxes);

                                    if let Stroke::BrushStroke(bs) = s {
                                        candidate_points.push(bs.path.start.pos);
                                        for seg in &bs.path.segments {
                                            match seg {
                                                Segment::LineTo { end } => {
                                                    candidate_points.push(end.pos)
                                                }
                                                Segment::QuadBezTo { end, .. } => {
                                                    candidate_points.push(end.pos)
                                                }
                                                Segment::CubBezTo { end, .. } => {
                                                    candidate_points.push(end.pos)
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            let passes_gate = !directly_colliding_keys.is_empty()
                                && (candidate_hitboxes.is_empty()
                                    || crate::pens::ScribbleDetector::passes_ink_gate_hitboxes(
                                        &stroke_points,
                                        &candidate_hitboxes,
                                        scribble_config.ink_gate_proximity_px,
                                        scribble_config.ink_gate_threshold,
                                    )
                                    || (!candidate_points.is_empty()
                                        && crate::pens::ScribbleDetector::passes_ink_gate(
                                            &stroke_points,
                                            &candidate_points,
                                            scribble_config.ink_gate_proximity_px,
                                            scribble_config.ink_gate_threshold,
                                        )));

                            if passes_gate {
                                let keys_to_trash = match scribble_config.mode {
                                    crate::pens::ScribbleMode::Stroke => directly_colliding_keys,
                                    crate::pens::ScribbleMode::Word => {
                                        let stroke_bounds_and_keys: Vec<(StrokeKey, Aabb)> =
                                            candidate_keys
                                                .iter()
                                                .filter_map(|&k| {
                                                    engine_view
                                                        .store
                                                        .get_stroke_ref(k)
                                                        .map(|s| (k, s.bounds()))
                                                })
                                                .collect();
                                        let clusters =
                                            crate::pens::scribble::WordGroup::cluster_words(
                                                &stroke_bounds_and_keys,
                                                18.0,
                                                1.0,
                                            );
                                        let mut trashing = Vec::new();
                                        for cluster in clusters {
                                            if cluster
                                                .keys
                                                .iter()
                                                .any(|k| directly_colliding_keys.contains(k))
                                            {
                                                trashing.extend(cluster.keys);
                                            }
                                        }
                                        if trashing.is_empty() {
                                            directly_colliding_keys
                                        } else {
                                            trashing
                                        }
                                    }
                                    crate::pens::ScribbleMode::Line => candidate_keys,
                                };

                                if !keys_to_trash.is_empty() {
                                    tracing::info!(
                                        "Scribble-to-erase: trashing {} stroke(s)",
                                        keys_to_trash.len()
                                    );
                                    engine_view.store.set_trashed_keys(&keys_to_trash, true);
                                    engine_view.store.remove_stroke(*current_stroke_key);
                                    self.state = BrushState::Idle;
                                    widget_flags |= engine_view.store.record(Instant::now());
                                    widget_flags.store_modified = true;
                                    widget_flags.redraw = true;
                                    widget_flags.resize = true;
                                    widget_flags.refresh_ui = true;
                                    return (
                                        EventResult {
                                            handled,
                                            propagate,
                                            progress: PenProgress::Finished,
                                        },
                                        widget_flags,
                                    );
                                }
                            }
                        }

                        // Check for Hold-to-Snap Shape (G2 & G12)
                        let shape_snap_config = engine_view
                            .config
                            .pens_config
                            .brush_config
                            .shape_snap_config
                            .clone();

                        let snapped_shape = if shape_snap_config.enable && stroke_points.len() >= 4
                        {
                            crate::pens::ShapeRecognizer::classify(
                                &stroke_points,
                                &shape_snap_config,
                            )
                        } else {
                            None
                        };

                        if let Some(snapped) = snapped_shape {
                            let shape = snapped.into_rnote_shape();
                            let current_style = engine_view
                                .config
                                .pens_config
                                .brush_config
                                .style_for_current_options();
                            let shape_stroke = Stroke::ShapeStroke(
                                crate::strokes::ShapeStroke::new(shape, current_style),
                            );

                            if let Some(stroke_slot) =
                                engine_view.store.get_stroke_mut(*current_stroke_key)
                            {
                                *stroke_slot = shape_stroke;
                            }

                            engine_view
                                .store
                                .update_geometry_for_stroke(*current_stroke_key);
                            engine_view.store.regenerate_rendering_for_stroke_threaded(
                                engine_view.tasks_tx.clone(),
                                *current_stroke_key,
                                engine_view.camera.viewport(),
                                engine_view.camera.image_scale(),
                            );
                            widget_flags |= engine_view
                                .document
                                .resize_autoexpand(engine_view.store, engine_view.camera);

                            self.state = BrushState::Idle;
                            widget_flags |= engine_view.store.record(Instant::now());
                            widget_flags.store_modified = true;
                            return (
                                EventResult {
                                    handled,
                                    propagate,
                                    progress: PenProgress::Finished,
                                },
                                widget_flags,
                            );
                        }

                        if let Some(Stroke::BrushStroke(brushstroke)) =
                            engine_view.store.get_stroke_mut(*current_stroke_key)
                        {
                            brushstroke.style = engine_view
                                .config
                                .pens_config
                                .brush_config
                                .style_for_current_options();
                        }

                        // Finish up the last stroke
                        engine_view
                            .store
                            .update_geometry_for_stroke(*current_stroke_key);
                        engine_view.store.regenerate_rendering_for_stroke_threaded(
                            engine_view.tasks_tx.clone(),
                            *current_stroke_key,
                            engine_view.camera.viewport(),
                            engine_view.camera.image_scale(),
                        );
                        widget_flags |= engine_view
                            .document
                            .resize_autoexpand(engine_view.store, engine_view.camera);

                        self.state = BrushState::Idle;

                        widget_flags |= engine_view.store.record(Instant::now());
                        widget_flags.store_modified = true;

                        PenProgress::Finished
                    }
                };

                EventResult {
                    handled,
                    propagate,
                    progress,
                }
            }
        };

        (event_result, widget_flags)
    }
}

impl DrawableOnDoc for Brush {
    fn bounds_on_doc(&self, engine_view: &EngineView) -> Option<Aabb> {
        let style = engine_view
            .config
            .pens_config
            .brush_config
            .style_for_current_options();

        match &self.state {
            BrushState::Idle => {
                if let Some(hover) = self.hover_element {
                    let width = style.stroke_width();
                    let radius = (width * 0.5).max(2.0);
                    Some(Aabb::from_half_extents(
                        hover.pos,
                        p2d::math::Vector2::new(radius + 4.0, radius + 4.0),
                    ))
                } else {
                    None
                }
            }
            BrushState::Drawing { path_builder, .. } => {
                path_builder.bounds(&style, engine_view.camera.zoom())
            }
        }
    }

    fn draw_on_doc(
        &self,
        cx: &mut piet_cairo::CairoRenderContext,
        engine_view: &EngineView,
    ) -> anyhow::Result<()> {
        cx.save().map_err(|e| anyhow::anyhow!("{e:?}"))?;

        match &self.state {
            BrushState::Idle => {
                if let Some(hover) = self.hover_element {
                    use rnote_compose::ext::Vector2Ext;
                    let style = engine_view
                        .config
                        .pens_config
                        .brush_config
                        .style_for_current_options();
                    let width = style.stroke_width();
                    let radius = (width * 0.5).max(1.5);
                    let outline_width = 1.0 / engine_view.camera.total_zoom();
                    let circle = kurbo::Circle::new(hover.pos.to_kurbo_point(), radius);
                    let ring_color = piet::Color::rgba8(120, 120, 120, 180);
                    cx.stroke(circle, &ring_color, outline_width);
                }
            }
            BrushState::Drawing {
                path_builder,
                preview_style,
                ..
            } => {
                match engine_view.config.pens_config.brush_config.style {
                    BrushStyle::Marker => {
                        // Don't draw the marker, as the pen would render on top of other strokes, while the stroke itself would render underneath them.
                    }
                    BrushStyle::Solid | BrushStyle::Textured => {
                        path_builder.draw_styled(
                            cx,
                            preview_style,
                            engine_view.camera.total_zoom(),
                        );
                    }
                }
            }
        }

        cx.restore().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

impl Brush {
    const INPUT_OVERSHOOT: f64 = 30.0;

    fn get_preview_style(engine_view: &EngineView) -> Style {
        let mut style = engine_view
            .config
            .pens_config
            .brush_config
            .style_for_current_options();

        if let Some(mut stroke_color) = style.stroke_color() {
            stroke_color.a = 1.0;
            style.set_stroke_color(stroke_color);
        }

        style
    }
}

#[cfg(feature = "ui")]
fn play_marker_sound(engine_view: &mut EngineViewMut) {
    if let Some(audioplayer) = engine_view.audioplayer {
        audioplayer.play_random_marker_sound();
    }
}

#[cfg(feature = "ui")]
fn trigger_brush_sound(engine_view: &mut EngineViewMut) {
    if let Some(audioplayer) = engine_view.audioplayer.as_mut() {
        audioplayer.trigger_random_brush_sound();
    }
}

fn new_builder(
    builder_type: PenPathBuilderType,
    element: Element,
    now: Instant,
) -> Box<dyn Buildable<Emit = Segment>> {
    match builder_type {
        PenPathBuilderType::Simple => Box::new(PenPathSimpleBuilder::start(element, now)),
        PenPathBuilderType::Curved => Box::new(PenPathCurvedBuilder::start(element, now)),
        PenPathBuilderType::Modeled => Box::new(PenPathModeledBuilder::start(element, now)),
    }
}
