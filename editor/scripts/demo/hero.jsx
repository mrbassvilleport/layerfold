// README title animation, made in Layerfold itself. Timings follow the short-form motion rules:
// one thing moves at a time, entrances decelerate, and the end holds long enough to read.
var ROOT = ROOT_DIR;
var W = 1280, H = 320, FPS = 30, DUR = 5.0;
var comp = app.project.items.addComp("Hero", W, H, 1, DUR, FPS);
comp.layers.addSolid([0.0902, 0.1020, 0.1686], "Backdrop", W, H, 1, DUR);

function arrive(start, seconds) {
    return "var k = Math.max(0, Math.min(1, (time - " + start + ") / " + seconds + ")); k = 1 - Math.pow(1 - k, 3); ";
}

var icon = comp.layers.add(app.project.importFile(new ImportOptions(new File(ROOT + "assets/app-icon/effectcraft-1024.png"))));
icon.name = "Icon";
icon.transform.position.setValue([226, H / 2]);
icon.transform.scale.expression = arrive(0.2, 0.45) + "var s = 15.5 + 2.0 * k; [s, s];";
icon.transform.opacity.expression = arrive(0.2, 0.3) + "100 * k;";

function animated(text, size, tracking, color, x, y, start, stagger, rise) {
    var T = comp.layers.addText(text);
    T.name = text;
    var td = T.property("ADBE Text Properties").property("ADBE Text Document"), d = td.value;
    d.font = "Inter-Bold"; d.fontSize = size; d.fillColor = color; d.applyFill = true; d.applyStroke = false;
    d.justification = ParagraphJustification.LEFT_JUSTIFY; d.tracking = tracking;
    td.setValue(d);
    T.transform.position.setValue([x, y]);
    var an = T.property("ADBE Text Properties").property("ADBE Text Animators").addProperty("ADBE Text Animator");
    an.name = "Rise";
    var pr = an.property("ADBE Text Animator Properties");
    pr.addProperty("ADBE Text Opacity").setValue(0);
    pr.addProperty("ADBE Text Position 3D").setValue([0, rise, 0]);
    pr.addProperty("ADBE Text Blur").setValue([6, 6]);
    var es = an.property("ADBE Text Selectors").addProperty("ADBE Text Expressible Selector");
    es.property("basedOn").setValue(1);
    es.property("amount").expression = "var s = " + start + " + (textIndex - 1) * " + stagger + "; var k = (time - s) / 0.3; " +
        "k = Math.max(0, Math.min(1, k)); k = 1 - Math.pow(1 - k, 3); var v = 100 * (1 - k); [v, v, v];";
    return T;
}

animated("Layerfold", 132, -10, [0.957, 0.937, 0.890], 350, 176, 0.65, 0.045, 46);
animated("Layer groups, snapping and slip on the timeline", 28, 0, [0.949, 0.710, 0.267], 356, 232, 1.45, 0.012, 14);
