// Windows Script Host JScript, invoked read-only by the Node harness.
// PID is validated before invocation; this prints no process arguments/env.
var pid = WScript.Arguments(0);
if (!/^\d+$/.test(pid)) WScript.Quit(2);
var query = GetObject('winmgmts:\\\\.\\root\\cimv2').ExecQuery(
  'SELECT ProcessId,ExecutablePath,CreationDate FROM Win32_Process WHERE ProcessId=' + pid
);
var entries = new Enumerator(query);
for (; !entries.atEnd(); entries.moveNext()) {
  var entry = entries.item();
  WScript.Echo(entry.ProcessId + '|' + entry.ExecutablePath + '|' + entry.CreationDate);
}
