//! Deterministic force layout for the relation network (ProseCraft `graph-layout.ts`, upstream
//! `bcb884d`): Fruchterman–Reingold per connected component, seeded from Mulberry32, components
//! packed apart, overlaps pushed to `min_distance`. The same graph always gives the same
//! picture.

use std::collections::HashMap;
use std::f64::consts::PI;

use super::graph::Point;

pub struct LayoutNode<'a> {
    pub id: &'a str,
    /// A starting point when there is no previous position.
    pub at: Option<Point>,
    /// Stays where `previous` (or `at`) puts it.
    pub pinned: bool,
}

pub struct LayoutEdge<'a> {
    pub from: &'a str,
    pub to: &'a str,
    /// Multiplies the attraction; 1 when `None`.
    pub weight: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub seed: Option<f64>,
    pub iterations: Option<f64>,
    pub min_distance: Option<f64>,
    pub previous: HashMap<String, Point>,
}

/// `screen = world * scale + translate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub scale: f64,
    pub translate: Point,
}

const DEFAULT_WIDTH: f64 = 800.0;
const DEFAULT_HEIGHT: f64 = 600.0;
const DEFAULT_ITERATIONS: f64 = 160.0;
const MIN_FIT_SCALE: f64 = 0.25;
const MAX_FIT_SCALE: f64 = 3.0;
const DEFAULT_SEED: f64 = 1.0;
const DEFAULT_MIN_DISTANCE: f64 = 34.0;
const COLLISION_ROUNDS: usize = 24;

fn golden_angle() -> f64 {
    PI * (3.0 - 5f64.sqrt())
}

fn positive(value: Option<f64>, fallback: f64) -> f64 {
    value
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(fallback)
}

fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// `x >>> 0`.
fn to_u32(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc().rem_euclid(4_294_967_296.0)) as u32
}

/// Mulberry32.
struct Random(u32);

impl Random {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut v = self.0;
        v = (v ^ (v >> 15)).wrapping_mul(v | 1);
        v ^= v.wrapping_add((v ^ (v >> 7)).wrapping_mul(v | 61));
        f64::from(v ^ (v >> 14)) / 4_294_967_296.0
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    fn visit(&mut self, p: Point) {
        self.min_x = self.min_x.min(p.x);
        self.min_y = self.min_y.min(p.y);
        self.max_x = self.max_x.max(p.x);
        self.max_y = self.max_y.max(p.y);
    }
}

struct Component {
    members: Vec<usize>,
    positions: HashMap<usize, Point>,
}

impl Component {
    fn bounds(&self) -> Bounds {
        let mut b = Bounds::empty();
        for i in &self.members {
            b.visit(self.positions[i]);
        }
        b
    }

    fn translate(&mut self, dx: f64, dy: f64) {
        for p in self.positions.values_mut() {
            p.x += dx;
            p.y += dy;
        }
    }
}

/// `layoutGraph`: a position for every node (by id).
pub fn layout(
    nodes: &[LayoutNode],
    edges: &[LayoutEdge],
    options: &Options,
) -> HashMap<String, Point> {
    let width = positive(options.width, DEFAULT_WIDTH);
    let height = positive(options.height, DEFAULT_HEIGHT);
    let iterations = positive(options.iterations, DEFAULT_ITERATIONS)
        .round()
        .max(0.0) as usize;
    let min_distance = positive(options.min_distance, DEFAULT_MIN_DISTANCE);
    let mut random = Random(to_u32(options.seed.unwrap_or(DEFAULT_SEED)));

    let mut index_by_id: HashMap<&str, usize> = HashMap::new();
    let mut unique: Vec<&LayoutNode> = Vec::new();
    for node in nodes {
        if node.id.is_empty() || index_by_id.contains_key(node.id) {
            continue;
        }
        index_by_id.insert(node.id, unique.len());
        unique.push(node);
    }
    let count = unique.len();
    let mut result = HashMap::new();
    if count == 0 {
        return result;
    }

    let mut links: Vec<(usize, usize, f64)> = Vec::new();
    let mut neighbors: Vec<Vec<usize>> = vec![Vec::new(); count];
    for edge in edges {
        let (Some(&from), Some(&to)) = (index_by_id.get(edge.from), index_by_id.get(edge.to))
        else {
            continue;
        };
        if from == to {
            continue;
        }
        let weight = edge
            .weight
            .filter(|w| w.is_finite())
            .map_or(1.0, |w| w.clamp(0.05, 8.0));
        links.push((from, to, weight));
        neighbors[from].push(to);
        neighbors[to].push(from);
    }

    let mut component_of: Vec<Option<usize>> = vec![None; count];
    let mut components: Vec<Vec<usize>> = Vec::new();
    for start in 0..count {
        if component_of[start].is_some() {
            continue;
        }
        let id = components.len();
        let mut members = Vec::new();
        let mut stack = vec![start];
        component_of[start] = Some(id);
        while let Some(current) = stack.pop() {
            members.push(current);
            for &n in &neighbors[current] {
                if component_of[n].is_none() {
                    component_of[n] = Some(id);
                    stack.push(n);
                }
            }
        }
        members.sort_unstable();
        components.push(members);
    }

    let ga = golden_angle();
    let mut points: Vec<Point> = Vec::with_capacity(count);
    let mut pinned = vec![false; count];
    let mut previous_points: Vec<Option<Point>> = vec![None; count];
    for (index, node) in unique.iter().enumerate() {
        let explicit = options
            .previous
            .get(node.id)
            .copied()
            .filter(|p| finite(*p))
            .or(node.at.filter(|p| finite(*p)));
        let start = explicit.unwrap_or_else(|| {
            // ProseCraft's order of operations, left to right: the simulation magnifies any
            // difference in the last bit.
            let i = index as f64;
            let scale = ((i + 0.5) / count as f64).sqrt();
            let x =
                libm::cos(i * ga) * width.min(height) * 0.35 * scale + (random.next() - 0.5) * 4.0;
            let y =
                libm::sin(i * ga) * width.min(height) * 0.35 * scale + (random.next() - 0.5) * 4.0;
            Point { x, y }
        });
        points.push(start);
        previous_points[index] = explicit;
        pinned[index] = node.pinned && explicit.is_some();
    }

    let mut layouts: Vec<Component> = Vec::new();
    for members in components {
        let component = component_of[members[0]];
        let local: Vec<(usize, usize, f64)> = links
            .iter()
            .filter(|(from, _, _)| component_of[*from] == component)
            .map(|&(from, to, w)| {
                let at = |n: usize| members.iter().position(|m| *m == n).unwrap_or(0);
                (at(from), at(to), w)
            })
            .collect();
        let settled = members.iter().any(|i| previous_points[*i].is_some());
        run_forces(
            &members,
            &mut points,
            &pinned,
            &previous_points,
            &local,
            &Forces {
                width,
                height,
                iterations,
                min_distance,
                settled,
            },
        );
        let positions = members.iter().map(|i| (*i, points[*i])).collect();
        layouts.push(Component { members, positions });
    }

    assign_targets(&mut layouts, &previous_points, min_distance, width);

    let mut ordered = vec![Point::default(); count];
    for layout in &layouts {
        for i in &layout.members {
            ordered[*i] = layout.positions[i];
        }
    }
    separate_overlaps(&mut ordered, &pinned, min_distance);
    for (index, node) in unique.iter().enumerate() {
        result.insert(node.id.to_owned(), ordered[index]);
    }
    result
}

struct Forces {
    width: f64,
    height: f64,
    iterations: usize,
    min_distance: f64,
    settled: bool,
}

fn run_forces(
    members: &[usize],
    points: &mut [Point],
    pinned: &[bool],
    anchors: &[Option<Point>],
    links: &[(usize, usize, f64)],
    o: &Forces,
) {
    let count = members.len();
    if count < 2 {
        return;
    }
    let ideal = ((o.width * o.height) / count as f64)
        .sqrt()
        .max(o.min_distance * 1.4)
        .min(o.width.min(o.height) / 5.0);
    let temperature_start = if o.settled {
        ideal * 0.12
    } else {
        o.width.min(o.height) / 5.0
    };
    let gravity = 0.012;
    let anchor_strength = 0.12;
    let mut moves_x = vec![0.0; count];
    let mut moves_y = vec![0.0; count];
    let mut center_x = 0.0;
    let mut center_y = 0.0;
    for &i in members {
        center_x += points[i].x;
        center_y += points[i].y;
    }
    center_x /= count as f64;
    center_y /= count as f64;

    for step in 0..o.iterations {
        moves_x.fill(0.0);
        moves_y.fill(0.0);
        for first in 0..count {
            let a = points[members[first]];
            for second in first + 1..count {
                let b = points[members[second]];
                let mut dx = a.x - b.x;
                let mut dy = a.y - b.y;
                let mut distance = (dx * dx + dy * dy).sqrt();
                if distance < 1e-6 {
                    dx = 0.0;
                    dy = 1.0;
                    distance = 1.0;
                }
                let force = (ideal * ideal) / distance;
                let ux = (dx / distance) * force;
                let uy = (dy / distance) * force;
                moves_x[first] += ux;
                moves_y[first] += uy;
                moves_x[second] -= ux;
                moves_y[second] -= uy;
            }
        }
        for &(first, second, weight) in links {
            let a = points[members[first]];
            let b = points[members[second]];
            let dx = a.x - b.x;
            let dy = a.y - b.y;
            let distance = (dx * dx + dy * dy).sqrt().max(1e-6);
            let force = ((distance * distance) / ideal) * weight;
            let px = (dx / distance) * force;
            let py = (dy / distance) * force;
            moves_x[first] -= px;
            moves_y[first] -= py;
            moves_x[second] += px;
            moves_y[second] += py;
        }
        let temperature = temperature_start * (1.0 - step as f64 / o.iterations as f64);
        for position in 0..count {
            let index = members[position];
            if pinned[index] {
                continue;
            }
            let point = &mut points[index];
            moves_x[position] -= (point.x - center_x) * gravity;
            moves_y[position] -= (point.y - center_y) * gravity;
            if let Some(anchor) = anchors[index] {
                moves_x[position] += (anchor.x - point.x) * anchor_strength;
                moves_y[position] += (anchor.y - point.y) * anchor_strength;
            }
            let length = (moves_x[position].powi(2) + moves_y[position].powi(2)).sqrt();
            if length < 1e-9 {
                continue;
            }
            let step_length = length.min(temperature) / length;
            point.x += moves_x[position] * step_length;
            point.y += moves_y[position] * step_length;
        }
    }
}

fn bounds_of(layouts: &[&Component]) -> Bounds {
    let mut b = Bounds::empty();
    for l in layouts {
        let c = l.bounds();
        b.min_x = b.min_x.min(c.min_x);
        b.min_y = b.min_y.min(c.min_y);
        b.max_x = b.max_x.max(c.max_x);
        b.max_y = b.max_y.max(c.max_y);
    }
    b
}

fn assign_targets(
    layouts: &mut [Component],
    previous: &[Option<Point>],
    min_distance: f64,
    width: f64,
) {
    let gap = min_distance * 2.0;
    let row_limit = width.max(min_distance * 10.0);
    let mut anchored: Vec<usize> = Vec::new();
    let mut packed: Vec<usize> = Vec::new();
    let mut anchors: HashMap<usize, Point> = HashMap::new();
    for (n, layout) in layouts.iter().enumerate() {
        let known: Vec<Point> = layout.members.iter().filter_map(|i| previous[*i]).collect();
        if known.is_empty() {
            packed.push(n);
            continue;
        }
        anchored.push(n);
        let len = known.len() as f64;
        anchors.insert(
            n,
            Point {
                x: known.iter().fold(0.0, |s, p| s + p.x) / len,
                y: known.iter().fold(0.0, |s, p| s + p.y) / len,
            },
        );
    }

    let (mut cursor_x, mut cursor_y, mut row_height) = (0.0, 0.0, 0.0f64);
    for &n in &packed {
        let b = layouts[n].bounds();
        let w = b.max_x - b.min_x;
        let h = b.max_y - b.min_y;
        if cursor_x > 0.0 && cursor_x + w > row_limit {
            cursor_y += row_height + gap;
            cursor_x = 0.0;
            row_height = 0.0;
        }
        layouts[n].translate(cursor_x - b.min_x, cursor_y - b.min_y);
        cursor_x += w + gap;
        row_height = row_height.max(h);
    }

    if !packed.is_empty() && !anchored.is_empty() {
        let pb = bounds_of(&packed.iter().map(|n| &layouts[*n]).collect::<Vec<_>>());
        let ab = bounds_of(&anchored.iter().map(|n| &layouts[*n]).collect::<Vec<_>>());
        let sx = ab.max_x + gap - pb.min_x;
        let sy = (ab.min_y + ab.max_y) / 2.0 - (pb.min_y + pb.max_y) / 2.0;
        for &n in &packed {
            layouts[n].translate(sx, sy);
        }
    } else if !packed.is_empty() {
        let b = bounds_of(&packed.iter().map(|n| &layouts[*n]).collect::<Vec<_>>());
        let sx = -(b.min_x + b.max_x) / 2.0;
        let sy = -(b.min_y + b.max_y) / 2.0;
        for &n in &packed {
            layouts[n].translate(sx, sy);
        }
    }

    for &n in &anchored {
        let layout = &layouts[n];
        let known: Vec<usize> = layout
            .members
            .iter()
            .copied()
            .filter(|i| previous[*i].is_some())
            .collect();
        let (cx, cy) = known.iter().fold((0.0, 0.0), |(x, y), i| {
            (x + layout.positions[i].x, y + layout.positions[i].y)
        });
        let len = known.len() as f64;
        let anchor = anchors.get(&n).copied().unwrap_or_default();
        layouts[n].translate(anchor.x - cx / len, anchor.y - cy / len);
    }
}

fn separate_overlaps(positions: &mut [Point], pinned: &[bool], min_distance: f64) {
    for _ in 0..COLLISION_ROUNDS {
        let mut moved = false;
        for first in 0..positions.len() {
            for second in first + 1..positions.len() {
                if pinned[first] && pinned[second] {
                    continue;
                }
                let a = positions[first];
                let b = positions[second];
                let mut dx = a.x - b.x;
                let mut dy = a.y - b.y;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance >= min_distance {
                    continue;
                }
                if distance < 1e-6 {
                    dx = 0.0;
                    dy = 1.0;
                } else {
                    dx /= distance;
                    dy /= distance;
                }
                let push = min_distance - distance.min(min_distance);
                if pinned[first] {
                    positions[second].x -= dx * push;
                    positions[second].y -= dy * push;
                } else if pinned[second] {
                    positions[first].x += dx * push;
                    positions[first].y += dy * push;
                } else {
                    positions[first].x += (dx * push) / 2.0;
                    positions[first].y += (dy * push) / 2.0;
                    positions[second].x -= (dx * push) / 2.0;
                    positions[second].y -= (dy * push) / 2.0;
                }
                moved = true;
            }
        }
        if !moved {
            return;
        }
    }
}

/// `fitView`: the transform that fits every point in the viewport with `padding` around it.
pub fn fit_view(points: &[Point], width: f64, height: f64, padding: f64) -> View {
    let width = positive(Some(width), DEFAULT_WIDTH);
    let height = positive(Some(height), DEFAULT_HEIGHT);
    let inset = if padding.is_finite() {
        padding.max(0.0)
    } else {
        0.0
    };
    let mut b = Bounds::empty();
    let mut any = false;
    for p in points.iter().filter(|p| finite(**p)) {
        b.visit(*p);
        any = true;
    }
    if !any {
        return View {
            scale: 1.0,
            translate: Point {
                x: width / 2.0,
                y: height / 2.0,
            },
        };
    }
    let span_x = b.max_x - b.min_x;
    let span_y = b.max_y - b.min_y;
    let available_x = (width - inset * 2.0).max(1.0);
    let available_y = (height - inset * 2.0).max(1.0);
    let mut raw = f64::INFINITY;
    if span_x > 0.0 {
        raw = raw.min(available_x / span_x);
    }
    if span_y > 0.0 {
        raw = raw.min(available_y / span_y);
    }
    if raw == f64::INFINITY {
        raw = 1.0;
    }
    let scale = raw.clamp(MIN_FIT_SCALE, MAX_FIT_SCALE);
    View {
        scale,
        translate: Point {
            x: width / 2.0 - ((b.min_x + b.max_x) / 2.0) * scale,
            y: height / 2.0 - ((b.min_y + b.max_y) / 2.0) * scale,
        },
    }
}

/// `screenToWorld`.
pub fn screen_to_world(point: Point, view: View) -> Point {
    Point {
        x: (point.x - view.translate.x) / view.scale,
        y: (point.y - view.translate.y) / view.scale,
    }
}

/// `clusterByKey`: starting positions with each group's members spiralling out from a point
/// on a ring (groups in UTF-16 order).
pub fn cluster_by_key(
    nodes: &[(String, Option<String>)],
    radius: Option<f64>,
    spacing: Option<f64>,
) -> HashMap<String, Point> {
    let mut names: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&str>> = HashMap::new();
    for (id, key) in nodes {
        let Some(key) = key.as_ref().filter(|k| !k.is_empty()) else {
            continue;
        };
        groups
            .entry(key.clone())
            .or_insert_with(|| {
                names.push(key.clone());
                Vec::new()
            })
            .push(id);
    }
    let mut result = HashMap::new();
    if names.is_empty() {
        return result;
    }
    names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    let n = names.len() as f64;
    let ring = positive(radius, 220f64.max(n * 110.0));
    let spacing = positive(spacing, DEFAULT_MIN_DISTANCE * 1.6);
    let ga = golden_angle();
    for (group_index, name) in names.iter().enumerate() {
        let g = group_index as f64;
        let angle = (g / n) * PI * 2.0 - PI / 2.0;
        let (cx, cy) = if names.len() == 1 {
            (0.0, 0.0)
        } else {
            (libm::cos(angle) * ring, libm::sin(angle) * ring)
        };
        for (index, id) in groups[name].iter().enumerate() {
            let i = index as f64;
            let r = spacing * (i + 0.5).sqrt();
            let theta = i * ga + g * 0.7;
            result.insert(
                (*id).to_owned(),
                Point {
                    x: cx + libm::cos(theta) * r,
                    y: cy + libm::sin(theta) * r,
                },
            );
        }
    }
    result
}
