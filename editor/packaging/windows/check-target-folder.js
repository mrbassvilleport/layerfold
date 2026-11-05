// MSI custom action (JScript, immediate): marks INSTALLFOLDER taken when it already holds an
// effectcraft.exe that this package didn't install (no BUILD.txt of our installed build). Looks at
// the folder itself, so junctions and 8.3 spellings of another EffectCraft's folder are caught.
// The folder our installed copy registered (PREVIOUSINSTALLFOLDER) is ours unless it or a parent
// has become a junction or other reparse point: a repair or upgrade restores a deleted BUILD.txt instead of
// refusing. Errors are not caught: the action is Return="check", so a folder that can't be
// inspected stops the install instead of passing.
function norm(p) {
  p = String(p || "").toLowerCase();
  return p && p.charAt(p.length - 1) != "\\" ? p + "\\" : p;
}
var REPARSE_POINT = 1024;
var dir = Session.Property("INSTALLFOLDER");
var fso = new ActiveXObject("Scripting.FileSystemObject");
// The folder and every parent up to the drive root: a junction anywhere on the path could lead
// the registered spelling into another program's tree.
function plainPath(p) {
  for (var f = fso.GetFolder(p); f; f = f.IsRootFolder ? null : f.ParentFolder) {
    if (f.Attributes & REPARSE_POINT) return false;
  }
  return true;
}
var registered = dir && norm(dir) == norm(Session.Property("PREVIOUSINSTALLFOLDER")) &&
  fso.FolderExists(dir) && plainPath(dir);
if (dir && !registered && fso.FileExists(dir + "effectcraft.exe")) {
  var ours = false;
  if (fso.FileExists(dir + "BUILD.txt")) {
    var f = fso.OpenTextFile(dir + "BUILD.txt", 1);
    var text = f.AtEndOfStream ? "" : f.ReadAll();
    f.Close();
    ours = text.indexOf("modified build (installed)") >= 0;
  }
  if (!ours) Session.Property("TARGETTAKEN") = "1";
}
