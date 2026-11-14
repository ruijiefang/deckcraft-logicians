//! Arrows as commutative diagrams draw them: curves between node boxes, parallel offsets,
//! squiggles, and the tips at their ends (arrowheads, hooks, bars).
//!
//! Pure geometry in the caller's units with y pointing down (slide points). Tips are proportioned
//! to `em`, the diagram's font size. "Left" means left of the direction of travel as seen on the
//! page: above a left-to-right arrow.

use kurbo::{BezPath, CubicBez, ParamCurve, ParamCurveArclen, ParamCurveDeriv, Point, Rect, Vec2};

/// Control arms of a bent curve, per unit of its chord.
pub const BEND_ARM: f64 = 0.3915;
/// Arclength accuracy, in the caller's units.
const ACCURACY: f64 = 1e-3;
/// The most teeth a squiggle has.
const MAX_TEETH: usize = 2_000;

/// `v` scaled to length 1, or `None` when it has no direction.
pub fn unit(v: Vec2) -> Option<Vec2> {
    let l = v.hypot();
    (l.is_finite() && l > 1e-12).then(|| v / l)
}

/// `v` turned `deg` degrees counterclockwise as seen on the page.
pub fn turn(v: Vec2, deg: f64) -> Vec2 {
    let a = -deg.to_radians();
    let (s, c) = a.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// The unit normal on the left of direction `d`.
pub fn left_of(d: Vec2) -> Vec2 {
    Vec2::new(d.y, -d.x)
}

fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Where a ray from `from`, inside `r`, in direction `dir` leaves `r`. `from` itself when the ray
/// has no direction or starts outside.
pub fn exit_rect(r: Rect, from: Point, dir: Vec2) -> Point {
    let mut t = f64::INFINITY;
    if dir.x > 1e-12 {
        t = t.min((r.x1 - from.x) / dir.x);
    } else if dir.x < -1e-12 {
        t = t.min((r.x0 - from.x) / dir.x);
    }
    if dir.y > 1e-12 {
        t = t.min((r.y1 - from.y) / dir.y);
    } else if dir.y < -1e-12 {
        t = t.min((r.y0 - from.y) / dir.y);
    }
    if t.is_finite() && t >= 0.0 { from + dir * t } else { from }
}

/// Where a ray from the centre `c` of an ellipse with radii `rx`, `ry` in direction `dir` leaves it.
pub fn exit_ellipse(c: Point, rx: f64, ry: f64, dir: Vec2) -> Point {
    let Some(d) = unit(dir) else { return c };
    if !(rx > 0.0 && ry > 0.0) {
        return c;
    }
    let k = ((d.x / rx).powi(2) + (d.y / ry).powi(2)).sqrt();
    if k > 1e-12 && k.is_finite() { c + d / k } else { c }
}

/// A curve from `a` to `b` leaving `a` along `out` and reaching `b` from `back` (the direction
/// from `b` toward the curve), with arms `arm` times the chord. Straight when both directions
/// lie along the chord.
pub fn curve(a: Point, out: Vec2, b: Point, back: Vec2, arm: f64) -> CubicBez {
    let d = (b - a).hypot();
    let len = if d.is_finite() { d * arm } else { 0.0 };
    let o = unit(out).unwrap_or(Vec2::ZERO);
    let k = unit(back).unwrap_or(Vec2::ZERO);
    CubicBez::new(a, a + o * len, b + k * len, b)
}

/// A straight segment as a curve.
pub fn line(a: Point, b: Point) -> CubicBez {
    CubicBez::new(a, a.lerp(b, 1.0 / 3.0), a.lerp(b, 2.0 / 3.0), b)
}

/// The curve's point at `t` (clamped to 0..1) and its unit tangent there.
pub fn at(c: &CubicBez, t: f64) -> (Point, Vec2) {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.5 };
    let p = c.eval(t);
    let d = c.deriv().eval(t).to_vec2();
    let tangent = unit(d).or_else(|| unit(c.p3 - c.p0)).unwrap_or(Vec2::new(1.0, 0.0));
    (p, tangent)
}

/// The curve's length.
pub fn length(c: &CubicBez) -> f64 {
    let l = c.arclen(ACCURACY);
    if l.is_finite() { l } else { 0.0 }
}

/// The part of `c` between `from_start` along it and `from_end` before its end; `None` when
/// nothing is left.
pub fn trim(c: &CubicBez, from_start: f64, from_end: f64) -> Option<CubicBez> {
    let total = length(c);
    let (s, e) = (from_start.max(0.0), from_end.max(0.0));
    if !(s.is_finite() && e.is_finite()) || s + e >= total - 1e-9 {
        return None;
    }
    if s == 0.0 && e == 0.0 {
        return Some(*c);
    }
    let t0 = if s > 0.0 { c.inv_arclen(s, ACCURACY) } else { 0.0 };
    let t1 = if e > 0.0 { c.inv_arclen(total - e, ACCURACY) } else { 1.0 };
    (t0.is_finite() && t1.is_finite() && t1 > t0).then(|| c.subsegment(t0..t1))
}

/// `c` moved `d` to its left: each control point moves along the normal of the polygon edge it
/// belongs to (close to the true offset for the gentle curves of diagrams).
pub fn offset(c: &CubicBez, d: f64) -> CubicBez {
    let chord = unit(c.p3 - c.p0).unwrap_or(Vec2::new(1.0, 0.0));
    let n = |a: Point, b: Point| left_of(unit(b - a).unwrap_or(chord));
    let n01 = n(c.p0, c.p1);
    let n23 = n(c.p2, c.p3);
    CubicBez::new(c.p0 + n01 * d, c.p1 + n01 * d, c.p2 + n23 * d, c.p3 + n23 * d)
}

/// `n + 1` points along `c`, for hit testing.
pub fn samples(c: &CubicBez, n: usize) -> Vec<Point> {
    let n = n.clamp(1, 256);
    (0..=n).map(|i| c.eval(i as f64 / n as f64)).filter(|p| finite(*p)).collect()
}

/// A circle as a path.
pub fn circle(c: Point, r: f64) -> BezPath {
    use kurbo::Shape as _;
    if finite(c) && r.is_finite() && r > 0.0 { kurbo::Circle::new(c, r).to_path(0.01) } else { BezPath::new() }
}

/// The curve as a path.
pub fn path_of(c: &CubicBez) -> BezPath {
    let mut p = BezPath::new();
    if finite(c.p0) && finite(c.p1) && finite(c.p2) && finite(c.p3) {
        p.move_to(c.p0);
        p.curve_to(c.p1, c.p2, c.p3);
    }
    p
}

/// A zigzag along `c`: straight for `lead` at each end, between them teeth `half` long reaching
/// `amp` to either side.
pub fn squiggle(c: &CubicBez, lead: f64, half: f64, amp: f64) -> BezPath {
    let total = length(c);
    let mut p = BezPath::new();
    if !(total > 0.0 && half > 0.0 && lead >= 0.0) || !finite(c.p0) {
        return path_of(c);
    }
    let point_at = |s: f64| -> (Point, Vec2) {
        let t = c.inv_arclen(s.clamp(0.0, total), ACCURACY);
        at(c, t)
    };
    p.move_to(c.p0);
    let span = total - 2.0 * lead;
    let teeth = if span > 0.0 { ((span / half).floor() as usize).min(MAX_TEETH) } else { 0 };
    if teeth < 2 {
        p.line_to(c.p3);
        return p;
    }
    // Teeth fill the middle evenly.
    let step = span / teeth as f64;
    let (start, _) = point_at(lead);
    p.line_to(start);
    for i in 1..teeth {
        let (q, d) = point_at(lead + step * i as f64);
        let side = if i % 2 == 1 { 1.0 } else { -1.0 };
        p.line_to(q + left_of(d) * (amp * side));
    }
    let (end, _) = point_at(total - lead);
    p.line_to(end);
    p.line_to(c.p3);
    p
}

/// What sits at an end of an arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tip {
    /// An arrowhead pointing along the arrow, its point at the end.
    Head,
    /// Two arrowheads, one behind the other (an epimorphism ↠).
    TwoHeads,
    /// Half an arrowhead, on the left (`true`) or the right of the line.
    Harpoon(bool),
    /// An arrowhead at the start pointing along the arrow (a monomorphism ↣).
    Mono,
    /// A hook at the start, curling to the left (`true`) or the right (↪).
    Hook(bool),
    /// A bar across the start (↦).
    Bar,
    /// The head of a double arrow (⇒).
    Implies,
    /// A bar across both lines of a double arrow.
    WideBar,
}

/// A drawn tip: its strokes, and how far from the arrow's end the line(s) should stop.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub path: BezPath,
    pub trim: f64,
}

/// Arrowhead length and half-width, in ems.
const HEAD_BACK: f64 = 0.21;
const HEAD_SPREAD: f64 = 0.24;
/// The second head of `TwoHeads`, behind the first, in ems.
const SECOND_HEAD: f64 = 0.143;
/// A hook's radius, in ems.
const HOOK: f64 = 0.098;
/// Half a bar, in ems.
const BAR: f64 = 0.163;
/// A double arrow's head: length, half-width; and where its lines stop, in ems.
const IMPLIES_BACK: f64 = 0.33;
const IMPLIES_SPREAD: f64 = 0.256;
const IMPLIES_TRIM: f64 = 0.2;
/// Half a bar across a double arrow, in ems.
const WIDE_BAR: f64 = 0.25;

/// One barb of an arrowhead from its back corner to its point `t`, bowing toward the line.
fn barb(p: &mut BezPath, t: Point, d: Vec2, n: Vec2, back: f64, spread: f64, side: f64) {
    p.move_to(t - d * back + n * (spread * side));
    p.quad_to(t - d * (back * 0.43) + n * (spread * 0.25 * side), t);
}

/// The tip `kind` at an arrow's end `at`, the arrow running along `dir` there. For the start's
/// tips (`Mono`, `Hook`, `Bar`, `WideBar`) `at` is the start; a reversed head at the start is a
/// `Head` with `dir` reversed. `lw` is the line width.
pub fn tip(kind: Tip, at: Point, dir: Vec2, em: f64, lw: f64) -> Drawn {
    let d = unit(dir).unwrap_or(Vec2::new(1.0, 0.0));
    let n = left_of(d);
    let half = lw.max(0.0) / 2.0;
    let mut p = BezPath::new();
    if !(finite(at) && em.is_finite() && em > 0.0) {
        return Drawn { path: p, trim: 0.0 };
    }
    let trim = match kind {
        Tip::Head | Tip::TwoHeads | Tip::Harpoon(_) => {
            let point = at - d * half;
            match kind {
                Tip::Harpoon(left) => barb(&mut p, point, d, n, HEAD_BACK * em, HEAD_SPREAD * em, if left { 1.0 } else { -1.0 }),
                _ => {
                    barb(&mut p, point, d, n, HEAD_BACK * em, HEAD_SPREAD * em, 1.0);
                    barb(&mut p, point, d, n, HEAD_BACK * em, HEAD_SPREAD * em, -1.0);
                }
            }
            if kind == Tip::TwoHeads {
                let second = point - d * (SECOND_HEAD * em);
                barb(&mut p, second, d, n, HEAD_BACK * em, HEAD_SPREAD * em, 1.0);
                barb(&mut p, second, d, n, HEAD_BACK * em, HEAD_SPREAD * em, -1.0);
            }
            half
        }
        Tip::Mono => {
            let point = at + d * (HEAD_BACK * em + half);
            barb(&mut p, point, d, n, HEAD_BACK * em, HEAD_SPREAD * em, 1.0);
            barb(&mut p, point, d, n, HEAD_BACK * em, HEAD_SPREAD * em, -1.0);
            HEAD_BACK * em + half
        }
        Tip::Hook(left) => {
            let r = HOOK * em;
            let side = if left { 1.0 } else { -1.0 };
            let bulge = 1.25 * r;
            let start = at + d * (bulge + half);
            let end = start + n * (2.0 * r * side);
            let k = bulge / 0.75;
            p.move_to(start);
            p.curve_to(start - d * k, end - d * k, end);
            bulge + half
        }
        Tip::Bar | Tip::WideBar => {
            let h = if kind == Tip::Bar { BAR } else { WIDE_BAR } * em;
            let c = at + d * half;
            p.move_to(c + n * h);
            p.line_to(c - n * h);
            half
        }
        Tip::Implies => {
            let point = at - d * half;
            barb(&mut p, point, d, n, IMPLIES_BACK * em, IMPLIES_SPREAD * em, 1.0);
            barb(&mut p, point, d, n, IMPLIES_BACK * em, IMPLIES_SPREAD * em, -1.0);
            IMPLIES_TRIM * em
        }
    };
    Drawn { path: p, trim }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::Shape as _;

    fn close(a: Point, b: Point) -> bool {
        (a - b).hypot() < 1e-6
    }

    #[test]
    fn turning_left_is_up_on_the_page() {
        let v = turn(Vec2::new(1.0, 0.0), 90.0);
        assert!(v.x.abs() < 1e-9 && (v.y + 1.0).abs() < 1e-9, "{v:?}");
        assert_eq!(left_of(Vec2::new(1.0, 0.0)), Vec2::new(0.0, -1.0));
        assert_eq!(left_of(Vec2::new(0.0, 1.0)), Vec2::new(1.0, 0.0), "the left of a downward arrow is the page's right");
    }

    #[test]
    fn rays_leave_boxes_and_ellipses_on_their_border() {
        let r = Rect::new(0.0, 0.0, 10.0, 4.0);
        assert!(close(exit_rect(r, Point::new(5.0, 2.0), Vec2::new(1.0, 0.0)), Point::new(10.0, 2.0)));
        assert!(close(exit_rect(r, Point::new(5.0, 2.0), Vec2::new(1.0, 1.0)), Point::new(7.0, 4.0)));
        assert!(close(exit_rect(r, Point::new(5.0, 2.0), Vec2::ZERO), Point::new(5.0, 2.0)));
        assert!(close(exit_ellipse(Point::new(0.0, 0.0), 3.0, 2.0, Vec2::new(0.0, -5.0)), Point::new(0.0, -2.0)));
    }

    #[test]
    fn a_bent_curve_leaves_at_its_angle() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(100.0, 0.0);
        let c = curve(a, turn(Vec2::new(1.0, 0.0), 30.0), b, turn(Vec2::new(-1.0, 0.0), -30.0), BEND_ARM);
        let (_, t) = at(&c, 0.0);
        assert!((t.y / t.x + 30f64.to_radians().tan()).abs() < 1e-6, "leaves 30° up: {t:?}");
        let (mid, _) = at(&c, 0.5);
        assert!(mid.y < -10.0, "bends to the left (up): {mid:?}");
        assert!((length(&c) - 100.0).abs() < 15.0);
    }

    #[test]
    fn trimming_and_offsets() {
        let c = line(Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        let t = trim(&c, 2.0, 3.0).unwrap_or(c);
        assert!((t.p0 - Point::new(2.0, 0.0)).hypot() < 1e-2 && (t.p3 - Point::new(7.0, 0.0)).hypot() < 1e-2, "{t:?}");
        assert!(trim(&c, 6.0, 6.0).is_none(), "nothing left");
        let o = offset(&c, 2.0);
        assert!(close(o.p0, Point::new(0.0, -2.0)) && close(o.p3, Point::new(10.0, -2.0)), "left is up: {o:?}");
    }

    #[test]
    fn tips_point_where_they_should() {
        let em = 10.0;
        let h = tip(Tip::Head, Point::new(50.0, 0.0), Vec2::new(1.0, 0.0), em, 0.4);
        let b = h.path.bounding_box();
        assert!(b.x1 <= 50.0 + 1e-9 && b.x0 < 49.0 && b.height() > 4.0, "the head is behind its point: {b:?}");
        let hook = tip(Tip::Hook(true), Point::new(0.0, 0.0), Vec2::new(1.0, 0.0), em, 0.4);
        let hb = hook.path.bounding_box();
        assert!(hb.y1 <= 1e-9 && hb.y0 < -1.5, "a left hook curls up: {hb:?}");
        assert!(hook.trim > 1.0);
        let mono = tip(Tip::Mono, Point::new(0.0, 0.0), Vec2::new(1.0, 0.0), em, 0.4);
        assert!(mono.path.bounding_box().x0 >= -1e-9, "a tail sits after the start");
        let harp = tip(Tip::Harpoon(true), Point::new(10.0, 0.0), Vec2::new(1.0, 0.0), em, 0.4).path.bounding_box();
        assert!(harp.y1 <= 1e-9, "a left harpoon has its barb above");
    }

    #[test]
    fn squiggles_wave_and_short_ones_are_straight() {
        let c = line(Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        let s = squiggle(&c, 2.4, 1.84, 0.76);
        let b = s.bounding_box();
        assert!(b.height() > 1.0 && b.height() < 1.6, "{b:?}");
        let short = squiggle(&line(Point::new(0.0, 0.0), Point::new(5.0, 0.0)), 2.4, 1.84, 0.76);
        assert!(short.bounding_box().height() < 1e-9);
    }

    #[test]
    fn hostile_numbers_give_no_nan() {
        for v in [f64::NAN, f64::INFINITY, -1e300, 0.0] {
            let p = Point::new(v, v);
            let c = curve(p, Vec2::new(v, 1.0), Point::new(1.0, 1.0), Vec2::new(1.0, v), v);
            let _ = trim(&c, v, v);
            let _ = squiggle(&c, v, v, v);
            for k in [Tip::Head, Tip::TwoHeads, Tip::Mono, Tip::Hook(false), Tip::Bar, Tip::Implies, Tip::WideBar, Tip::Harpoon(true)] {
                let d = tip(k, p, Vec2::new(v, v), v, v);
                assert!(d.trim.is_finite());
            }
            assert!(samples(&c, 8).iter().all(|q| q.x.is_finite()));
        }
    }
}
