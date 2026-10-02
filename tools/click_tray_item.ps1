param([string]$ItemName = ($([char]0x9000) + $([char]0x51FA)))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -Namespace Win32 -Name Mouse -MemberDefinition @"
[DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint cButtons, System.IntPtr dwExtraInfo);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
"@ -ErrorAction SilentlyContinue
$RIGHTDOWN=0x0008; $RIGHTUP=0x0010; $LEFTDOWN=0x0002; $LEFTUP=0x0004

$root = [System.Windows.Automation.AutomationElement]::RootElement

function Find-FirstDesc($root, $prop, $val) {
    $c = New-Object System.Windows.Automation.PropertyCondition($prop, $val)
    return $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $c)
}

# locate the tray button by tooltip name, scoped to taskbar + overflow
$tray = Find-FirstDesc $root ([System.Windows.Automation.AutomationElement]::NameProperty) 'frpc-tray'
# filter: must be a small button-like element (ClassName ToolbarWindow32 button or Button)
$found = $null
if ($tray) {
    # walk: could be the toolbar itself; find the actual button
    $candidates = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,'frpc-tray')))
    foreach ($c in $candidates) {
        $cn = $c.Current.ClassName
        $r  = $c.Current.BoundingRectangle
        if ($cn -eq 'Button' -or $cn -eq '' -or $cn -eq 'ToolbarWindow32') {
            # prefer elements with reasonable button size
            if ($r.Width -gt 0 -and $r.Width -lt 60 -and $r.Height -gt 0 -and $r.Height -lt 60) {
                $found = $c; break
            }
        }
    }
}
if (-not $found) { Write-Host 'TRAY_NOT_FOUND'; exit 2 }
$r = $found.Current.BoundingRectangle
$X=[int]($r.X+$r.Width/2); $Y=[int]($r.Y+$r.Height/2)
Write-Host ("TRAY_FOUND x={0} y={1} class={2} w={3} h={4}" -f $X,$Y,$found.Current.ClassName,$r.Width,$r.Height)

# right-click
[Win32.Mouse]::SetCursorPos($X,$Y) | Out-Null
Start-Sleep -Milliseconds 150
[Win32.Mouse]::mouse_event($RIGHTDOWN,0,0,0,[IntPtr]::Zero)
[Win32.Mouse]::mouse_event($RIGHTUP,0,0,0,[IntPtr]::Zero)
Start-Sleep -Milliseconds 500

# find menu item
$menu=$null
for ($i=0;$i -lt 30;$i++) {
    $c2=New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,$ItemName)
    $menu=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$c2)
    if($menu){break}
    Start-Sleep -Milliseconds 100
}
if(-not $menu){Write-Host 'MENU_ITEM_NOT_FOUND';exit 3}
Write-Host ("MENU_ITEM_FOUND class={0}" -f $menu.Current.ClassName)
$pat=$null
if($menu.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pat)){
  $pat.Invoke(); Write-Host 'INVOKED'
}else{
  $cp=$menu.GetClickablePoint()
  [Win32.Mouse]::SetCursorPos([int]$cp.X,[int]$cp.Y)|Out-Null
  Start-Sleep -Milliseconds 80
  [Win32.Mouse]::mouse_event($LEFTDOWN,0,0,0,[IntPtr]::Zero)
  [Win32.Mouse]::mouse_event($LEFTUP,0,0,0,[IntPtr]::Zero)
  Write-Host 'CLICKED'
}
