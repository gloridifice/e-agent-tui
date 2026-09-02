# Ultimate E2E: run the real pie (debug build, same code as the installed
# fixed one) inside a REAL Windows Terminal window, inject a physical Ctrl+V
# with an image-only clipboard, then check whether pie wrote a temporary
# pi-clipboard-*.png (proof the whole chain ran: synthetic key -> ReadClipboard
# -> arboard image -> temp PNG -> paste).
param(
    [string]$Pie = "G:\MyProjects\Rust\e\target\debug\pie.exe",
    [int]$HoldMs = 300,
    [string]$Trace = "$env:TEMP\pie_paste_trace.log"
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class NativeKeys4 {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@

function Assert-Foreground([IntPtr]$hwnd) {
    $deadline = (Get-Date).AddSeconds(8)
    while ((Get-Date) -lt $deadline) {
        if ([NativeKeys4]::GetForegroundWindow() -eq $hwnd) { return }
        [NativeKeys4]::keybd_event(0xA4, 0, 0, [UIntPtr]::Zero)
        [NativeKeys4]::keybd_event(0xA4, 0, 2, [UIntPtr]::Zero)
        [void][NativeKeys4]::SetForegroundWindow($hwnd)
        Start-Sleep -Milliseconds 200
    }
    throw "could not focus the pie window"
}

# clean slate
Get-ChildItem "$env:TEMP\pi-clipboard-*.png" -ErrorAction SilentlyContinue | Remove-Item -Force
"temp pi-clipboard files cleaned"
Remove-Item $Trace -ErrorAction SilentlyContinue
# inherited by wt.exe -> pie.exe
$env:E_TUI_PASTE_TRACE = $Trace

# image-only clipboard
$bitmap = New-Object System.Drawing.Bitmap 4,3
$bitmap.SetPixel(0,0,[System.Drawing.Color]::Red)
$bitmap.SetPixel(1,1,[System.Drawing.Color]::Lime)
$bitmap.SetPixel(2,2,[System.Drawing.Color]::Blue)
[System.Windows.Forms.Clipboard]::SetImage($bitmap)
"clipboard image set"

# WT rewrites the window title from the app's OSC title (pie sets it to
# "pi"), so find the NEW WindowsTerminal process by comparing PIDs before/after.
$wtBefore = @(Get-Process WindowsTerminal -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
Start-Process wt.exe -ArgumentList '-w', '_new', '--title', 'PIETEST', $Pie

$target = $null
try {
    $deadline = (Get-Date).AddSeconds(20)
    $hwnd = [IntPtr]::Zero
    while ((Get-Date) -lt $deadline) {
        $candidate = Get-Process WindowsTerminal -ErrorAction SilentlyContinue |
            Where-Object { $wtBefore -notcontains $_.Id } |
            Select-Object -First 1
        if ($candidate -and $candidate.MainWindowHandle -ne 0) {
            $target = $candidate
            $hwnd = $candidate.MainWindowHandle
            break
        }
        Start-Sleep -Milliseconds 250
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw "pie window never appeared" }
    $target.Refresh()
    "pie window: pid=$($target.Id) title='$($target.MainWindowTitle)'"

    # let pie finish startup (pi RPC child boot)
    Start-Sleep -Seconds 6
    $pieProc = Get-Process pie -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Pie }
    if ($pieProc) { "pie process running: pid=$($pieProc.Id) started=$($pieProc.StartTime.ToString('HH:mm:ss'))" }
    else { "pie process NOT running" }
    $target.Refresh()
    "window title before focus: '$($target.MainWindowTitle)'"
    Assert-Foreground $hwnd

    # re-assert the image clipboard immediately before injecting; a human
    # copying text mid-test would otherwise invalidate the result silently
    $check = [System.Windows.Forms.Clipboard]::GetImage()
    if (-not $check) {
        $clipText = [System.Windows.Forms.Clipboard]::GetText()
        throw "clipboard is NOT an image anymore (text: '$($clipText.Substring(0, [Math]::Min(40, $clipText.Length)))') - aborting injection"
    }
    "clipboard verified image right before injection: $($check.Width)x$($check.Height)"

    # physical Ctrl+V, held
    [NativeKeys4]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [NativeKeys4]::keybd_event(0x56, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds $HoldMs
    [NativeKeys4]::keybd_event(0x56, 0, 2, [UIntPtr]::Zero)
    [NativeKeys4]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
    "injected physical Ctrl+V (held ${HoldMs}ms)"
    Start-Sleep -Milliseconds 1500
    $after = [System.Windows.Forms.Clipboard]::GetImage()
    if ($after) { "clipboard still image after injection: $($after.Width)x$($after.Height)" }
    else { "WARNING: clipboard changed during injection (no image)" }

    "--- screenshot after image paste ---"
    $shot1 = cutty --pid $target.Id -r "min(0.5x, 540s)"
    $shot1

    # clear the composer with a physical Ctrl+C (delivered as 0x03 by WT)
    [NativeKeys4]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [NativeKeys4]::keybd_event(0x43, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 80
    [NativeKeys4]::keybd_event(0x43, 0, 2, [UIntPtr]::Zero)
    [NativeKeys4]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 600

    # text clipboard: must paste exactly once via the normal bracketed path
    Set-Clipboard -Value "double check text"
    Assert-Foreground $hwnd
    [NativeKeys4]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [NativeKeys4]::keybd_event(0x56, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds $HoldMs
    [NativeKeys4]::keybd_event(0x56, 0, 2, [UIntPtr]::Zero)
    [NativeKeys4]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
    "injected physical Ctrl+V with TEXT clipboard"
    Start-Sleep -Milliseconds 1200

    $pastedText = Get-ChildItem "$env:TEMP\pi-clipboard-*.png" -ErrorAction SilentlyContinue
    "temp pngs after text paste: $($pastedText.Count) (image paste wrote 1; a 2nd would mean a double read)"

    "--- screenshot after text paste ---"
    cutty --pid $target.Id -r "min(0.5x, 540s)"

    $pasted = Get-ChildItem "$env:TEMP\pi-clipboard-*.png" -ErrorAction SilentlyContinue
    if ($pasted) {
        "RESULT: PASTE CHAIN RAN - temp png written:"
        $pasted | ForEach-Object { "  $($_.Name) ($($_.Length) bytes)" }
    } else {
        "RESULT: NO TEMP PNG - the chain did not run"
    }

    $target.Refresh()
    "window title after paste: '$($target.MainWindowTitle)'"
} finally {
    if ($target) { Stop-Process -Id $target.Id -Force -ErrorAction SilentlyContinue }
    Get-Process pie -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Pie } | Stop-Process -Force -ErrorAction SilentlyContinue
}
