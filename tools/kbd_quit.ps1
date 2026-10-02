param([string]$ItemName = ($([char]0x9000) + $([char]0x51FA)))
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type -Namespace W -Name U -MemberDefinition @"
[DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, System.IntPtr dwExtraInfo);
"@

$VK_LWIN=0x5B; $VK_B=0x42; $VK_RIGHT=0x27; $VK_UP=0x26; $VK_DOWN=0x28; $VK_RETURN=0x0D; $VK_SHIFT=0x10; $VK_F10=0x79; $VK_ESCAPE=0x1B
$KEYDOWN=0x0000; $KEYUP=0x0002

function Send-Key($vk){ [W.U]::keybd_event($vk,0,$KEYDOWN,[IntPtr]::Zero); Start-Sleep -Milliseconds 40; [W.U]::keybd_event($vk,0,$KEYUP,[IntPtr]::Zero); Start-Sleep -Milliseconds 120 }

function FocusedName {
    $f=[System.Windows.Automation.AutomationElement]::FocusedElement
    if($f){return $f.Current.Name} else {return ''}
}

# 1. Win+B to focus tray
[W.U]::keybd_event($VK_LWIN,0,$KEYDOWN,[IntPtr]::Zero)
[W.U]::keybd_event($VK_B,0,$KEYDOWN,[IntPtr]::Zero)
[W.U]::keybd_event($VK_B,0,$KEYUP,[IntPtr]::Zero)
[W.U]::keybd_event($VK_LWIN,0,$KEYUP,[IntPtr]::Zero)
Start-Sleep -Milliseconds 500

# 2. arrow right up to ~40 times, find frpc-tray
$hit=$false
for($i=0;$i -lt 40;$i++){
  $nm=FocusedName
  if($nm -eq 'frpc-tray'){Write-Host ("FOUND at step {0}" -f $i);$hit=$true;break}
  Send-Key $VK_RIGHT
}
if(-not $hit){
  # maybe in overflow: press Enter to open overflow, then arrow
  Send-Key $VK_RETURN
  Start-Sleep -Milliseconds 400
  for($i=0;$i -lt 40;$i++){
    $nm=FocusedName
    if($nm -eq 'frpc-tray'){Write-Host ("FOUND(overflow) at step {0}" -f $i);$hit=$true;break}
    Send-Key $VK_RIGHT
  }
}
if(-not $hit){Write-Host 'NOT_FOUND'; exit 2}

# 3. open context menu (Shift+F10)
[W.U]::keybd_event($VK_SHIFT,0,$KEYDOWN,[IntPtr]::Zero)
[W.U]::keybd_event($VK_F10,0,$KEYDOWN,[IntPtr]::Zero)
[W.U]::keybd_event($VK_F10,0,$KEYUP,[IntPtr]::Zero)
[W.U]::keybd_event($VK_SHIFT,0,$KEYUP,[IntPtr]::Zero)
Start-Sleep -Milliseconds 500

# 4. find the menu item via UIA descendants, invoke it
$root=[System.Windows.Automation.AutomationElement]::RootElement
$menu=$null
for($j=0;$j -lt 30;$j++){
  $c=New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,$ItemName)
  $menu=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$c)
  if($menu){break}
  Start-Sleep -Milliseconds 100
}
if($menu){
  Write-Host ("MENU_FOUND class={0}" -f $menu.Current.ClassName)
  $pat=$null
  if($menu.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pat)){$pat.Invoke();Write-Host 'INVOKED'}
  else{
    $r=$menu.Current.BoundingRectangle
    [System.Windows.Forms.Cursor]::Position=New-Object System.Drawing.Point([int]($r.X+$r.Width/2),[int]($r.Y+$r.Height/2))
    Start-Sleep -Milliseconds 80
    [W.U]::keybd_event($VK_RETURN,0,$KEYDOWN,[IntPtr]::Zero); [W.U]::keybd_event($VK_RETURN,0,$KEYUP,[IntPtr]::Zero)
    Write-Host 'ENTERED'
  }
} else {
  Write-Host 'MENU_ITEM_NOT_FOUND (try keyboard down-nav)'
  # fallback: press Down a few times then Enter
  Send-Key $VK_DOWN; Send-Key $VK_DOWN; Send-Key $VK_DOWN; Send-Key $VK_DOWN; Send-Key $VK_DOWN
  Send-Key $VK_DOWN; Send-Key $VK_DOWN
  Send-Key $VK_RETURN
}
