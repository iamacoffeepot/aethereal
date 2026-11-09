//! A size asked of a window and the size winit answered it with (ADR-0047): whether the windowing system held the
//! window at its size rather than resize it.

#[cfg(test)]
mod tests;

use winit::dpi::{LogicalSize, PhysicalSize};

/// The size in pixels winit makes of `asked`, in logical pixels, on a screen of `scale` pixels to each: whole logical
/// pixels first, then those scaled and rounded, as winit's Wayland window rounds a size it applies.
pub fn physical(asked: LogicalSize<f64>, scale: f64) -> PhysicalSize<u32> {
    asked.cast::<u32>().to_physical(scale)
}

/// Whether a window `before` that size, asked to be `wanted`, was held at its size: winit answered `applied`, the size
/// it had, and not the size asked. A window asked for the size it has is not held, and nor is one the platform resized
/// to another size than asked, as one cut to the screen.
pub fn held(before: PhysicalSize<u32>, applied: PhysicalSize<u32>, wanted: PhysicalSize<u32>) -> bool {
    let unchanged = applied == before;
    let unasked = applied != wanted;

    unchanged && unasked
}
