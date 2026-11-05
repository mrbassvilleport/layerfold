// The project the README demos are filmed on: synthetic clips and text only.
var ROOT = ROOT_DIR;
var comp = app.project.items.addComp("Demo", 1280, 720, 1, 12, 30);

function clip(file, name, start, inP, outP) {
    var l = comp.layers.add(app.project.importFile(new ImportOptions(new File(ROOT + file))));
    l.name = name;
    l.startTime = start;
    l.inPoint = inP;
    l.outPoint = outP;
    l.transform.scale.setValue([200, 200]);
    return l;
}

function title(name, value, size, y, inP, outP) {
    var T = comp.layers.addText(value);
    T.name = name;
    var td = T.property("ADBE Text Properties").property("ADBE Text Document"), d = td.value;
    d.font = "Inter-Bold"; d.fontSize = size; d.fillColor = [1, 1, 1]; d.applyFill = true; d.applyStroke = false;
    d.justification = ParagraphJustification.CENTER_JUSTIFY;
    td.setValue(d);
    T.transform.position.setValue([640, y]);
    T.inPoint = inP;
    T.outPoint = outP;
    return T;
}

var bg = comp.layers.addSolid([0.09, 0.1, 0.17], "Backdrop", 1280, 720, 1, 12);
var tide = clip("tide.mp4", "Tide", 3, 6, 11);
var dawn = clip("dawn.mp4", "Dawn", 0, 1, 6);
var cap = title("Caption", "First light", 54, 620, 3, 6);
var ttl = title("Title", "Morning", 120, 360, 1.5, 4.5);

comp.openInViewer();
app.run("layer.group", { layers: [dawn.id, tide.id], name: "Shots" });
app.run("layer.group", { layers: [ttl.id, cap.id], name: "Titles" });
