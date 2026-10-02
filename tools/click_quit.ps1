# Click the "退出" item of frpc-tray tray menu via UI Automation.
# 1. find tray icon button named "frpc-tray" in the notification area (incl. overflow)
# 2. right-click it to open the context menu
# 3. find MenuItem "退出" and invoke it
# 4. report whether the frpc-tray process is still alive

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$ErrorActionPreference = 'Continue'

function Find-TrayButton {
    # search all descendants of the desktop for a Button/MenuItem named "frpc-tray"
    $root = [System.Windows.Automation.AutomationElement]::RootElement
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty, 'frpc-tray')
    # try TreeScope Descendants (slow but thorough)
    $el = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $cond)
    return $el
}

$btn = Find-TrayButton
if (-not $btn) { Write-Host 'tray button not found'; exit 2 }
Write-Host ('found tray element: ' + $btn.Current.Name + ' class=' + $btn.Current.ClassName)

# right-click it
$pt = $btn.Current.BoundingRectangle
$X = [int]($pt.X + $pt.Width/2)
$Y = [int]($pt.Y + $pt.Height/2)
Write-Host "right-click at $X,$Y"
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Cursor]::Position = New-Object System.Drawing.Point($X, $Y)
Start-Sleep -Milliseconds 200
# send right click via SendInput
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Mouse {
    [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint cButtons, IntPtr dwExtraInfo);
    public const uint RIGHTDOWN = 0x0008;
    public const uint RIGHTUP = 0x0010;
}
"@
[Mouse]::mouse_event([Mouse]::RIGHTDOWN, 0, 0, 0, [IntPtr]::Zero)
[Mouse]::mouse_event([Mouse]::RIGHTUP, 0, 0, 0, [IntPtr]::Zero)
Start-Sleep -Milliseconds 500

# find the 退出 menu item
$menu = $null
for ($i=0; $i -lt 20; $i++) {
    $cond2 = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty, '退出')
    $menu = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst(
        [System.Windows.Automation.TreeScope]::Descendants, $cond2)
    if ($menu) { break }
    Start-Sleep -Milliseconds 100
}
if (-not $menu) { Write-Host '退出 menu item not found'; exit 3 }
Write-Host ('found menu item: ' + $menu.Current.Name + ' class=' + $menu.Current.ClassName)
# invoke it
$pattern = $null
if ($menu.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    $pattern.Invoke()
    Write-Host 'invoked 退出'
} else {
    Write-Host 'no invoke pattern; trying click'
    $clickable = $menu.GetClickablePoint()
    [System.Windows.Forms.Cursor]::Position = [System.Drawing.Point]::new([int]$clickable.X, [int]$clickable.Y)
    [Mouse]::mouse_event([Mouse]::RIGHTDOWN,0,0,0,[IntPtr]::Zero)
    [Mouse]::mouse_event([Mouse]::RIGHTUP,0,0,0,[IntPtr]::Zero)
}
