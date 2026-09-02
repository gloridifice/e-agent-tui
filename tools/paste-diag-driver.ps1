# Diagnostic driver: run the paste_diag probe in a REAL Windows Terminal
# window, then inject a physical Ctrl+V with an image-only clipboard (the
# exact user scenario) and dump the probe's event log.
param(
    [string]$Probe = "G:\MyProjects\Rust\e\target\debug\examples\paste_diag.exe",
    [string]$Log = "$env:TEMP\paste_diag.log",
    [int]$HoldMs = 300
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class NativeKeys3 {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
}
'@

function Assert-Foreground([IntPtr]$hwnd) {
    $deadline = (Get-Date).AddSeconds(8)
    while ((Get-Date) -lt $deadline) {
        if ([NativeKeys3]::GetForegroundWindow() -eq $hwnd) { return }
        [NativeKeys3]::keybd_event(0xA4, 0, 0, [UIntPtr]::Zero)
        [NativeKeys3]::keybd_event(0xA4, 0, 2, [UIntPtr]::Zero)
        [void][NativeKeys3]::SetForegroundWindow($hwnd)
        Start-Sleep -Milliseconds 200
    }
    throw "could not focus the diagnostic window"
}

Remove-Item $Log -ErrorAction SilentlyContinue

# image-only clipboard
$bitmap = New-Object System.Drawing.Bitmap 4,3
$bitmap.SetPixel(0,0,[System.Drawing.Color]::Red)
$bitmap.SetPixel(1,1,[System.Drawing.Color]::Lime)
$bitmap.SetPixel(2,2,[System.Drawing.Color]::Blue)
[System.Windows.Forms.Clipboard]::SetImage($bitmap)
"clipboard image set: $([System.Windows.Forms.Clipboard]::GetImage().Width)x$([System.Windows.Forms.Clipboard]::GetImage().Height)"

Start-Process wt.exe -ArgumentList '-w', '_new', '--title', 'DIAGPROBE', $Probe, $Log

$target = $null
try {
    $deadline = (Get-Date).AddSeconds(20)
    $hwnd = [IntPtr]::Zero
    while ((Get-Date) -lt $deadline) {
        $candidate = Get-Process WindowsTerminal -ErrorAction SilentlyContinue |
            Where-Object { $_.MainWindowTitle -eq 'DIAGPROBE' } |
            Select-Object -First 1
        if ($candidate -and $candidate.MainWindowHandle -ne 0) {
            $target = $candidate
            $hwnd = $candidate.MainWindowHandle
            break
        }
        Start-Sleep -Milliseconds 250
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw "diagnostic window never appeared" }

    # wait for READY in the log
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline) {
        if ((Test-Path $Log) -and (Get-Content $Log -ErrorAction SilentlyContinue) -match 'READY') { break }
        Start-Sleep -Milliseconds 200
    }

    Assert-Foreground $hwnd

    # physical Ctrl+V, held
    [NativeKeys3]::keybd_event(0x11, 0, 0, [UIntPtr]::Zero)
    [NativeKeys3]::keybd_event(0x56, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds $HoldMs
    [NativeKeys3]::keybd_event(0x56, 0, 2, [UIntPtr]::Zero)
    [NativeKeys3]::keybd_event(0x11, 0, 2, [UIntPtr]::Zero)
    "injected physical Ctrl+V (held ${HoldMs}ms)"
    Start-Sleep -Milliseconds 800

    "--- probe log ---"
    Get-Content $Log
} finally {
    if ($target) { Stop-Process -Id $target.Id -Force -ErrorAction SilentlyContinue }
    Get-Process paste_diag -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}
