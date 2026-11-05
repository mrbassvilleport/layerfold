//! Which layers in locked layer groups a removal or edit would change, read through parents,
//! track mattes, layer properties and expression references.

use super::{ItemId, Layer, LayerId, locked_by_group};

/// The first layer in a locked folder that removing `gone` (layers of comp `cid`) would change:
/// one of them, a layer of that comp whose parent, track matte or layer reference points at one of
/// them, or a layer of any comp whose expression may read one of them (`layer(…)` by name or index,
/// of `thisComp` or `comp("Name")`; references it can't resolve count).
pub(crate) fn locked_dependant<'a>(project: &'a effectcraft_project::Project, cid: ItemId, gone: &[LayerId]) -> Option<&'a Layer> {
    if gone.is_empty() {
        return None;
    }
    let comp = project.comp(cid)?;
    let comp_name = project.item(cid).map(|i| i.name.as_str()).unwrap_or_default();
    // 1-based index of the topmost removed layer: removing it shifts every index from there down.
    let first_gone = comp.layers.iter().position(|l| gone.contains(&l.id)).map(|i| i + 1);
    let mut hit = |k: ItemId, l: &Layer, refs: &[ExprLayerRef]| {
        (k == cid && gone.contains(&l.id))
            || refs.iter().any(|r| {
                let in_comp = match &r.comp {
                    ExprComp::This => k == cid,
                    ExprComp::Named(n) => n == comp_name,
                    ExprComp::Unknown => true,
                };
                in_comp
                    && match &r.layer {
                        // A named layer is visited itself; an index shifts when a layer above goes.
                        ExprLayer::Name(_) | ExprLayer::Comp => false,
                        ExprLayer::Index(i) => first_gone.is_some_and(|g| *i >= g),
                        ExprLayer::Unknown | ExprLayer::Count => true,
                    }
            })
    };
    let polluted = project_polluted(project);
    locked_layers(project).find(|(k, l)| (polluted && has_expression(l)) || reads_any(project, *k, l, &mut hit)).map(|(_, l)| l)
}

/// Every layer in a locked layer group, with its comp.
fn locked_layers(project: &effectcraft_project::Project) -> impl Iterator<Item = (ItemId, &Layer)> {
    project.comps().flat_map(|(id, c)| c.layers.iter().filter(|l| locked_by_group(c, l.id)).map(|l| (*id, l)))
}

/// Visits `l` and every layer it reads, at any depth: parents, track mattes, layer properties and
/// expression references (an unresolved one reads every layer it may name). Stops at the first
/// layer for which `hit(comp, layer, its expression references)` is true. Removing or changing a
/// layer anywhere in that chain changes `l`.
fn reads_any(project: &effectcraft_project::Project, cid: ItemId, l: &Layer, hit: &mut dyn FnMut(ItemId, &Layer, &[ExprLayerRef]) -> bool) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut todo = vec![(cid, l.id)];
    while let Some((k, id)) = todo.pop() {
        if !seen.insert((k, id)) {
            continue;
        }
        let Some(l) = project.comp(k).and_then(|c| c.layer(id)) else { continue };
        let mut refs = vec![];
        let mut linked = vec![];
        l.props.walk("", &mut |_, p| {
            for v in std::iter::once(&p.value).chain(p.keys.iter().map(|k| &k.value)) {
                if let effectcraft_keyframe::Value::Layer(Some(lid)) = v {
                    linked.push(LayerId(*lid));
                }
            }
            if let Some(e) = p.expr.as_ref().filter(|e| e.enabled) {
                refs.extend(expr_layer_refs(&e.text));
            }
        });
        if hit(k, l, &refs) {
            return true;
        }
        todo.extend(l.parent.iter().chain(l.track_matte.as_ref().map(|m| &m.layer)).chain(linked.iter()).map(|id| (k, *id)));
        for r in &refs {
            let comps: Vec<ItemId> = match &r.comp {
                ExprComp::This => vec![k],
                ExprComp::Named(n) => project.comps().filter(|(id, _)| project.item(**id).is_some_and(|i| &i.name == n)).map(|(id, _)| *id).collect(),
                ExprComp::Unknown => project.comps().map(|(id, _)| *id).collect(),
            };
            for kc in comps {
                let Some(c) = project.comp(kc) else { continue };
                match &r.layer {
                    ExprLayer::Name(n) => todo.extend(c.layers.iter().filter(|x| &x.name == n).map(|x| (kc, x.id))),
                    ExprLayer::Index(i) => todo.extend(i.checked_sub(1).and_then(|i| c.layers.get(i)).map(|x| (kc, x.id))),
                    ExprLayer::Unknown => todo.extend(c.layers.iter().map(|x| (kc, x.id))),
                    ExprLayer::Count | ExprLayer::Comp => {}
                }
            }
        }
    }
    false
}

/// A locked-group layer outside `doomed` that reads one of the `doomed` comps, directly or through
/// the layers it reads (by name, or through a reference that can't be resolved).
pub(crate) fn locked_reader_of_comps<'a>(project: &'a effectcraft_project::Project, doomed: &[ItemId]) -> Option<&'a Layer> {
    let names: Vec<&str> = doomed.iter().filter(|id| project.comp(**id).is_some()).filter_map(|id| project.item(*id)).map(|i| i.name.as_str()).collect();
    if names.is_empty() {
        return None;
    }
    let mut hit = |k: ItemId, _: &Layer, refs: &[ExprLayerRef]| {
        doomed.contains(&k)
            || refs.iter().any(|r| match &r.comp {
                ExprComp::This => false,
                ExprComp::Named(n) => names.contains(&n.as_str()),
                ExprComp::Unknown => true,
            })
    };
    let polluted = project_polluted(project);
    locked_layers(project)
        .filter(|(k, _)| !doomed.contains(k))
        .find(|(k, l)| (polluted && has_expression(l)) || reads_any(project, *k, l, &mut hit))
        .map(|(_, l)| l)
}

/// Which comp an expression's reference reads.
#[derive(Debug, PartialEq)]
pub(crate) enum ExprComp {
    /// `thisComp.layer(…)` (or a bare `layer(…)`).
    This,
    /// `comp("Name")…`.
    Named(String),
    /// Any other receiver (a variable, a computed comp name).
    Unknown,
}

/// What an expression reads from that comp.
#[derive(Debug, PartialEq)]
pub(crate) enum ExprLayer {
    Name(String),
    Index(usize),
    /// Computed (`layer(index - 1)`, `layer("R" + "ef")`, `["layer"]`): any removal may change it.
    Unknown,
    /// `numLayers`: any removal changes it.
    Count,
    /// `comp("Name")` itself: only deleting the comp matters.
    Comp,
}

#[derive(Debug, PartialEq)]
pub(crate) struct ExprLayerRef {
    pub comp: ExprComp,
    pub layer: ExprLayer,
}

/// An expression token. Comments and whitespace are dropped.
#[derive(Debug, PartialEq)]
enum Tok<'a> {
    Ident(&'a str),
    /// A string literal; `None` when its value isn't known (an escape other than a quote or a
    /// backslash, or unterminated).
    Str(Option<String>),
    Num(&'a str),
    /// A template literal: it can hold any code, so it reads anything.
    Template,
    /// A regular expression literal; `true` when its text names a route to other layers (then
    /// the scan may have misread a division, so it reads anything).
    Regex(bool),
    Punct(char),
}

/// Keywords after which an operand starts (the expression rewriter's `DELIM_KW` and `PREFIX_KW`).
fn is_keyword(w: &str) -> bool {
    matches!(
        w,
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "do"
            | "else"
            | "export"
            | "extends"
            | "finally"
            | "for"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "let"
            | "return"
            | "switch"
            | "throw"
            | "try"
            | "var"
            | "while"
            | "with"
            | "yield"
            | "of"
            | "typeof"
            | "void"
            | "delete"
            | "await"
            | "new"
    )
}

/// Whether a `/` after the tokens `out` starts a regular expression literal rather than a division.
fn regex_allowed(out: &[Tok]) -> bool {
    let n = out.len();
    // A postfix `++` / `--` ends a value: `x++ / y` divides.
    if let (Some(Tok::Punct(a)), Some(Tok::Punct(b))) = (n.checked_sub(1).and_then(|i| out.get(i)), n.checked_sub(2).and_then(|i| out.get(i)))
        && a == b
        && matches!(a, '+' | '-')
        && matches!(n.checked_sub(3).and_then(|i| out.get(i)), Some(Tok::Ident(_) | Tok::Num(_) | Tok::Punct(')' | ']')))
    {
        return false;
    }
    match n.checked_sub(1).and_then(|i| out.get(i)) {
        None => true,
        Some(Tok::Punct(c)) => !matches!(c, ')' | ']' | '}'),
        Some(Tok::Ident(w)) => is_keyword(w),
        Some(_) => false,
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn tokens(text: &str) -> Vec<Tok<'_>> {
    let mut out = vec![];
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let rest = text.get(i..).unwrap_or("");
        if c.is_whitespace() {
            continue;
        }
        if rest.starts_with("//") {
            while it.next_if(|(_, c)| *c != '\n').is_some() {}
        } else if rest.starts_with("/*") {
            it.next();
            let mut prev = ' ';
            for (_, c) in it.by_ref() {
                if prev == '*' && c == '/' {
                    break;
                }
                prev = c;
            }
        } else if c == '/' && regex_allowed(&out) {
            // `/…/flags`: a `/` inside `[…]` or after `\\` doesn't end it.
            let (mut class, mut escaped) = (false, false);
            let mut end = text.len();
            for (j, ch) in it.by_ref() {
                end = j;
                match ch {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '[' => class = true,
                    ']' => class = false,
                    '/' if !class => break,
                    '\n' => break,
                    _ => {}
                }
            }
            while it.next_if(|(_, ch)| is_ident(*ch)).is_some() {}
            let body = text.get(i..end).unwrap_or("");
            out.push(Tok::Regex(LAYER_ROUTES.iter().chain(GLOBALS.iter()).any(|w| body.contains(w))));
        } else if c == '"' || c == '\'' || c == '`' {
            let (mut s, mut known, mut escaped) = (String::new(), true, false);
            let mut closed = false;
            for (_, ch) in it.by_ref() {
                if escaped {
                    known &= ch == c || ch == '\\';
                    s.push(ch);
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == c {
                    closed = true;
                    break;
                } else {
                    s.push(ch);
                }
            }
            out.push(match c {
                '`' => Tok::Template,
                _ => Tok::Str(Some(s).filter(|_| known && closed)),
            });
        } else if c.is_ascii_digit() {
            let mut end = i + c.len_utf8();
            while let Some((j, ch)) = it.next_if(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '.') {
                end = j + ch.len_utf8();
            }
            out.push(Tok::Num(text.get(i..end).unwrap_or("")));
        } else if is_ident(c) {
            let mut end = i + c.len_utf8();
            while let Some((j, ch)) = it.next_if(|(_, ch)| is_ident(*ch)) {
                end = j + ch.len_utf8();
            }
            out.push(Tok::Ident(text.get(i..end).unwrap_or("")));
        } else {
            out.push(Tok::Punct(c));
        }
    }
    out
}

/// The comp read by the member access whose `.` or `[` is `toks[at]`.
fn receiver(toks: &[Tok], at: usize) -> ExprComp {
    let Some(j) = at.checked_sub(1) else { return ExprComp::Unknown };
    let member = |k: usize| k.checked_sub(1).and_then(|p| toks.get(p)) == Some(&Tok::Punct('.'));
    match toks.get(j) {
        Some(Tok::Ident("thisComp")) if !member(j) => ExprComp::This,
        Some(Tok::Punct(')')) => {
            // The matching `(`; `comp(` before it with exactly one literal argument names the comp.
            let mut depth = 0usize;
            let mut k = j;
            loop {
                match toks.get(k) {
                    Some(Tok::Punct(')')) => depth += 1,
                    Some(Tok::Punct('(')) => depth = depth.saturating_sub(1),
                    _ => {}
                }
                if depth == 0 {
                    break;
                }
                let Some(prev) = k.checked_sub(1) else { return ExprComp::Unknown };
                k = prev;
            }
            match (k.checked_sub(1).map(|f| (f, toks.get(f))), toks.get(k + 1..j)) {
                (Some((f, Some(Tok::Ident("comp")))), Some([Tok::Str(Some(n))])) if !member(f) => ExprComp::Named(n.clone()),
                _ => ExprComp::Unknown,
            }
        }
        _ => ExprComp::Unknown,
    }
}

/// The argument of the call whose `(` is `toks[open]`: one string literal or one integer.
fn literal_arg(toks: &[Tok], open: usize) -> ExprLayer {
    match toks.get(open + 1..open + 3) {
        Some([Tok::Str(Some(n)), Tok::Punct(')')]) => ExprLayer::Name(n.clone()),
        Some([Tok::Num(n), Tok::Punct(')')]) => n.parse().map_or(ExprLayer::Unknown, ExprLayer::Index),
        _ => ExprLayer::Unknown,
    }
}

/// Names that lead from an expression to other layers or comps. Any use of one in a form not
/// resolved below reads [`ExprLayer::Unknown`] (aliases, computed access), so callers stay safe.
const LAYER_ROUTES: [&str; 9] = ["thisComp", "comp", "layer", "numLayers", "activeCamera", "source", "containingComp", "eval", "constructor"];

/// Names of the global object or code constructors: through them any route can be reached.
const GLOBALS: [&str; 6] = ["globalThis", "window", "self", "global", "Function", "Reflect"];

/// The expression prelude's classes: constructed directly they wrap any layer or comp id.
const PRELUDE: [&str; 5] = ["Layer", "Comp", "Footage", "Markers", "Prop"];

/// Free names an expression may use without reaching other layers: the prelude's functions and
/// this layer's getters, and plain JavaScript values. Any other free name (a prelude internal, a
/// reflection helper, a name the prelude adds later) reads anything: an allowlist, because a
/// list of dangerous names is never complete. `thisComp`, `comp` and `layer` are resolved apart.
const SAFE_NAMES: &[&str] = &[
    "thisComp",
    "thisLayer",
    "thisProperty",
    "value",
    "time",
    "index",
    "name",
    "inPoint",
    "outPoint",
    "startTime",
    "width",
    "height",
    "hasParent",
    "parent",
    "hasVideo",
    "enabled",
    "active",
    "transform",
    "position",
    "scale",
    "rotation",
    "opacity",
    "anchorPoint",
    "text",
    "marker",
    "numKeys",
    "velocity",
    "speed",
    "colorDepth",
    "add",
    "sub",
    "mul",
    "div",
    "clamp",
    "dot",
    "cross",
    "length",
    "normalize",
    "degreesToRadians",
    "radiansToDegrees",
    "linear",
    "ease",
    "easeIn",
    "easeOut",
    "random",
    "gaussRandom",
    "seedRandom",
    "noise",
    "wiggle",
    "temporalWiggle",
    "smooth",
    "loopIn",
    "loopOut",
    "loopInDuration",
    "loopOutDuration",
    "key",
    "nearestKey",
    "valueAtTime",
    "velocityAtTime",
    "speedAtTime",
    "posterizeTime",
    "effect",
    "mask",
    "content",
    "footage",
    "createPath",
    "sourceRectAtTime",
    "selectorValue",
    "textIndex",
    "textTotal",
    "toComp",
    "fromComp",
    "toWorld",
    "fromWorld",
    "toCompVec",
    "fromCompVec",
    "toWorldVec",
    "fromWorldVec",
    "lookAt",
    "framesToTime",
    "timeToFrames",
    "timeToTimecode",
    "timeToCurrentFormat",
    "timeToFeetAndFrames",
    "hslToRgb",
    "rgbToHsl",
    "hexToRgb",
    "Math",
    "Number",
    "String",
    "Array",
    "Boolean",
    "Date",
    "RegExp",
    "JSON",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "undefined",
    "NaN",
    "Infinity",
    "true",
    "false",
    "null",
    "function",
    "arguments",
];

/// Members that hand back their object or run code with another receiver (`thisComp.valueOf()`,
/// `L.call(C, 2)`): a route can hide behind them.
const UNSAFE_MEMBERS: [&str; 7] = ["valueOf", "call", "apply", "bind", "constructor", "prototype", "toJSON"];

/// Whether a declaration of `w` may stand for the expression's own variable. Scope and order
/// aren't tracked (`var Object;` leaves the built-in in place, a parameter binds only inside its
/// function), so a name that may be a real global never counts as declared: JavaScript's and the
/// prelude's globals are capitalised (`Object`, `Reflect`, `Layer`), apart from `thisProject` and
/// those caught before (`eval`, `globalThis`, `__…`).
fn shadowable(w: &str) -> bool {
    !w.starts_with(|c: char| c.is_ascii_uppercase()) && w != "thisProject"
}

/// Whether the expression may change what the shared expression globals mean for later
/// expressions (every expression runs in one JavaScript realm): it assigns to a safe free name
/// or its members (`effect = …`, `Math.sin = …`), or reads anything.
fn pollutes(text: &str) -> bool {
    let toks = tokens(text);
    let assigns_safe = toks.iter().enumerate().any(|(i, t)| {
        let is_assign = *t == Tok::Punct('=')
            && !matches!(toks.get(i + 1), Some(Tok::Punct('=' | '>')))
            && !matches!(i.checked_sub(1).and_then(|p| toks.get(p)), Some(Tok::Punct('=' | '!' | '<' | '>')));
        if !is_assign {
            return false;
        }
        // The root of the assigned reference: `a.b[c].d = …` → `a`.
        let mut k = i;
        let mut root = None;
        while let Some(p) = k.checked_sub(1) {
            match toks.get(p) {
                Some(Tok::Ident(w)) => {
                    root = Some(*w);
                    match p.checked_sub(1).and_then(|q| toks.get(q)) {
                        Some(Tok::Punct('.')) => k = p.saturating_sub(1),
                        _ => break,
                    }
                }
                Some(Tok::Punct(']' | ')')) => {
                    let mut depth = 0usize;
                    let mut q = p;
                    loop {
                        match toks.get(q) {
                            Some(Tok::Punct(']' | ')')) => depth += 1,
                            Some(Tok::Punct('[' | '(')) => depth = depth.saturating_sub(1),
                            _ => {}
                        }
                        if depth == 0 {
                            break;
                        }
                        let Some(n) = q.checked_sub(1) else { break };
                        q = n;
                    }
                    k = q;
                }
                // `x += 1`: the operator before `=`.
                Some(Tok::Punct('+' | '-' | '*' | '/' | '%' | '&' | '|' | '^')) if p + 1 == i => k = p,
                _ => break,
            }
        }
        root.is_some_and(|w| SAFE_NAMES.contains(&w) || !shadowable(w))
    });
    assigns_safe || expr_layer_refs(text).contains(&ExprLayerRef { comp: ExprComp::Unknown, layer: ExprLayer::Unknown })
}

/// Whether some enabled expression in the project [`pollutes`] the shared globals: then what any
/// locked layer's expression reads can't be known.
fn project_polluted(project: &effectcraft_project::Project) -> bool {
    project.comps().any(|(_, c)| {
        c.layers.iter().any(|l| {
            let mut hit = false;
            l.props.walk("", &mut |_, p| hit |= p.expr.as_ref().is_some_and(|e| e.enabled && pollutes(&e.text)));
            hit
        })
    })
}

/// Whether `l` has an enabled expression.
fn has_expression(l: &Layer) -> bool {
    let mut hit = false;
    l.props.walk("", &mut |_, p| hit |= p.expr.as_ref().is_some_and(|e| e.enabled));
    hit
}

/// The names an expression declares itself (variables, functions, parameters, assignments), which
/// shadow free names. Destructuring declares nothing here (it is read as Unknown).
fn declared<'a>(toks: &[Tok<'a>]) -> std::collections::HashSet<&'a str> {
    let mut out = std::collections::HashSet::new();
    let at = |i: usize| toks.get(i);
    let before = |i: usize| i.checked_sub(1).and_then(|p| toks.get(p));
    // Plain names inside the parentheses whose `(` is `open` (parameter lists).
    let params = |open: usize, out: &mut std::collections::HashSet<&'a str>| {
        let mut depth = 0usize;
        for (j, t) in toks.iter().enumerate().skip(open) {
            match t {
                Tok::Punct('(' | '[' | '{') => depth += 1,
                Tok::Punct(')' | ']' | '}') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                Tok::Ident(w) if depth == 1 && matches!(before(j), Some(Tok::Punct('(' | ','))) => {
                    out.insert(*w);
                }
                _ => {}
            }
        }
    };
    let (mut decl, mut depth) = (false, 0usize);
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Ident("var" | "let" | "const") => (decl, depth) = (true, 0),
            Tok::Punct('(' | '[' | '{') if decl => depth += 1,
            Tok::Punct(')' | ']' | '}') if decl => match depth.checked_sub(1) {
                Some(d) => depth = d,
                None => decl = false,
            },
            Tok::Punct(';') => decl = false,
            Tok::Ident("function" | "catch") => {
                let open = match at(i + 1) {
                    Some(Tok::Ident(n)) => {
                        out.insert(*n);
                        i + 2
                    }
                    _ => i + 1,
                };
                if at(open) == Some(&Tok::Punct('(')) {
                    params(open, &mut out);
                }
            }
            Tok::Ident(w) => {
                let assigned = at(i + 1) == Some(&Tok::Punct('=')) && at(i + 2) != Some(&Tok::Punct('=')) && before(i) != Some(&Tok::Punct('.'));
                let listed = decl && depth == 0 && matches!(before(i), Some(Tok::Ident("var" | "let" | "const") | Tok::Punct(',')));
                let looped = matches!(at(i + 1), Some(Tok::Ident("in" | "of"))) && before(i) == Some(&Tok::Punct('('));
                if assigned || listed || looped {
                    out.insert(*w);
                }
            }
            // `(a, b) => …`: the parameters of an arrow function.
            Tok::Punct('=') if at(i + 1) == Some(&Tok::Punct('>')) && before(i) == Some(&Tok::Punct(')')) => {
                let mut depth = 0usize;
                let mut k = i.saturating_sub(1);
                loop {
                    match toks.get(k) {
                        Some(Tok::Punct(')')) => depth += 1,
                        Some(Tok::Punct('(')) => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    if depth == 0 {
                        break;
                    }
                    let Some(p) = k.checked_sub(1) else { break };
                    k = p;
                }
                params(k, &mut out);
            }
            _ => {}
        }
    }
    out
}

/// Whether a `[` after `prev` indexes a value (rather than starting an array literal).
fn indexes(prev: Option<&Tok>) -> bool {
    match prev {
        Some(Tok::Ident(w)) => !is_keyword(w),
        Some(Tok::Punct(c)) => matches!(c, ')' | ']'),
        Some(Tok::Str(_) | Tok::Num(_) | Tok::Template | Tok::Regex(_)) => true,
        None => false,
    }
}

/// The layer and comp references of an expression, without evaluating it. Forms it can't
/// resolve read [`ExprComp::Unknown`] / [`ExprLayer::Unknown`], so callers stay conservative.
pub(crate) fn expr_layer_refs(text: &str) -> Vec<ExprLayerRef> {
    let toks = tokens(text);
    let own = declared(&toks);
    let mut out = vec![];
    let r = |comp, layer| ExprLayerRef { comp, layer };
    let at = |i: usize| toks.get(i);
    for (i, t) in toks.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|p| toks.get(p));
        let dot = prev == Some(&Tok::Punct('.'));
        let next = at(i + 1);
        let call = next == Some(&Tok::Punct('('));
        // What follows a comp value: `.member` and `[…]` are resolved where they appear.
        let member_next = |j: usize| matches!(at(j), Some(Tok::Punct('.' | '[')));
        match t {
            Tok::Ident("layer") if call => out.push(r(if dot { receiver(&toks, i.saturating_sub(1)) } else { ExprComp::This }, literal_arg(&toks, i + 1))),
            // `thisComp.layer` taken as a value can be called with anything later.
            Tok::Ident("layer") if dot => out.push(r(receiver(&toks, i.saturating_sub(1)), ExprLayer::Unknown)),
            Tok::Ident("numLayers") if dot => out.push(r(receiver(&toks, i.saturating_sub(1)), ExprLayer::Count)),
            // The active camera is whichever camera layer is on top: any removal may change it.
            Tok::Ident("activeCamera") if dot => out.push(r(receiver(&toks, i.saturating_sub(1)), ExprLayer::Unknown)),
            // A layer's source or containing comp, unless only its own members are read.
            Tok::Ident("source" | "containingComp") if dot && !member_next(i + 1) => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            Tok::Ident("thisComp") if !member_next(i + 1) => out.push(r(ExprComp::This, ExprLayer::Unknown)),
            Tok::Ident("constructor") if dot => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            Tok::Ident(w) if GLOBALS.contains(w) || PRELUDE.contains(w) || w.starts_with("__") => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            // `\u…` in a name (`thisComp.\u006cayer`) can spell any route.
            Tok::Punct('\\') => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            Tok::Regex(true) => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            Tok::Ident("comp") if call && !dot => {
                let named = match toks.get(i + 2..i + 4) {
                    Some([Tok::Str(Some(n)), Tok::Punct(')')]) => Some(n.clone()),
                    _ => None,
                };
                let aliased = named.is_none() || !member_next(i + 4);
                let comp = || named.clone().map_or(ExprComp::Unknown, ExprComp::Named);
                out.push(r(comp(), ExprLayer::Comp));
                if aliased {
                    out.push(r(comp(), ExprLayer::Unknown));
                }
            }
            Tok::Ident("comp") => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            Tok::Ident("eval" | "Function") | Tok::Template => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            // `obj["key"]`, `obj[0]`: a literal key that leads nowhere reads nothing; a computed key
            // (`obj[k]`) may name any route, so it reads anything.
            Tok::Punct('[') if indexes(i.checked_sub(1).and_then(|p| toks.get(p))) => {
                let closed = at(i + 2) == Some(&Tok::Punct(']'));
                let layer = match next {
                    Some(Tok::Num(_)) if closed => continue,
                    Some(Tok::Str(Some(k))) if closed && !LAYER_ROUTES.contains(&k.as_str()) => continue,
                    Some(Tok::Str(Some(k))) if closed && k == "numLayers" => ExprLayer::Count,
                    _ => ExprLayer::Unknown,
                };
                out.push(r(receiver(&toks, i), layer));
            }
            // `with (comp) layer`, `var {layer: L} = comp`: names bound out of sight.
            Tok::Ident("with") => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            Tok::Ident("var" | "let" | "const") if matches!(next, Some(Tok::Punct('{' | '['))) => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            Tok::Ident(w) if dot && (UNSAFE_MEMBERS.contains(w) || w.starts_with("__")) => out.push(r(ExprComp::Unknown, ExprLayer::Unknown)),
            // An object literal's key (`{a: 1, b: 2}`) is no name read.
            Tok::Ident(_) if matches!(prev, Some(Tok::Punct('{' | ','))) && next == Some(&Tok::Punct(':')) => {}
            Tok::Ident(w) if !(dot || is_keyword(w) || SAFE_NAMES.contains(w) || (own.contains(w) && shadowable(w))) => {
                out.push(r(ExprComp::Unknown, ExprLayer::Unknown))
            }
            // A route spelled as a string (`getOwnPropertyDescriptor(…, "containingComp")`).
            Tok::Str(s)
                if prev != Some(&Tok::Punct('['))
                    && s.as_deref().is_none_or(|s| LAYER_ROUTES.contains(&s) || GLOBALS.contains(&s) || PRELUDE.contains(&s) || s.starts_with("__")) =>
            {
                out.push(r(ExprComp::Unknown, ExprLayer::Unknown));
            }
            _ => {}
        }
    }
    out
}
