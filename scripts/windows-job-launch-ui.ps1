param([string]$Client, [string]$Resources, [string]$Scratch, [string]$Out, [string]$Cause, [switch]$CompileOnly)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Drawing, WindowsBase
Add-Type -ReferencedAssemblies @(
    [System.Windows.Automation.AutomationElement].Assembly.Location,
    [System.Windows.Automation.TreeScope].Assembly.Location,
    [System.Drawing.Bitmap].Assembly.Location,
    [System.Windows.Rect].Assembly.Location
) -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Imaging;
using System.Text;
using System.Threading;
using System.Windows.Automation;
public static class JobFailureUi {
    public static string Wait(uint pid, string cause, string screenshot, int timeout) {
        using (var changed = new AutoResetEvent(false)) {
            StructureChangedEventHandler structure = (sender, args) => changed.Set();
            AutomationPropertyChangedEventHandler property = (sender, args) => changed.Set();
            var root = AutomationElement.RootElement;
            Automation.AddStructureChangedEventHandler(root, TreeScope.Subtree, structure);
            Automation.AddAutomationPropertyChangedEventHandler(root, TreeScope.Subtree, property,
                AutomationElement.NameProperty, AutomationElement.IsOffscreenProperty);
            try {
                var clock = Stopwatch.StartNew();
                do {
                    var windows = root.FindAll(TreeScope.Children,
                        new PropertyCondition(AutomationElement.ProcessIdProperty, (int)pid));
                    foreach (AutomationElement window in windows) {
                        if (window.Current.IsOffscreen) continue;
                        var text = new StringBuilder(window.Current.Name);
                        var elements = window.FindAll(TreeScope.Descendants, Condition.TrueCondition);
                        foreach (AutomationElement element in elements) {
                            if (element.Current.IsOffscreen) continue;
                            text.AppendLine(element.Current.Name);
                            object pattern;
                            if (element.TryGetCurrentPattern(TextPattern.Pattern, out pattern))
                                text.AppendLine(((TextPattern)pattern).DocumentRange.GetText(4096));
                            if (text.Length > 65536) throw new InvalidOperationException("Native UI text exceeded bound");
                        }
                        if (!text.ToString().Contains(cause)) continue;
                        var rect = window.Current.BoundingRectangle;
                        if (rect.Width < 1 || rect.Height < 1) throw new InvalidOperationException("Failure window has no visible geometry");
                        using (var bitmap = new Bitmap((int)rect.Width, (int)rect.Height)) {
                            using (var graphics = Graphics.FromImage(bitmap))
                                graphics.CopyFromScreen((int)rect.X, (int)rect.Y, 0, 0, bitmap.Size);
                            bitmap.Save(screenshot, ImageFormat.Png);
                        }
                        return text.ToString();
                    }
                    int remaining = timeout - (int)clock.ElapsedMilliseconds;
                    if (remaining <= 0 || !changed.WaitOne(remaining))
                        throw new TimeoutException("Native startup UI did not expose the containment cause");
                } while (true);
            } finally {
                Automation.RemoveStructureChangedEventHandler(root, structure);
                Automation.RemoveAutomationPropertyChangedEventHandler(root, property);
            }
        }
    }
}
'@
if ($CompileOnly) { return }

$instance = 'job-ui-' + [Guid]::NewGuid().ToString('N').Substring(0, 10)
$homePath = Join-Path $Scratch 'native-ui'
$data = Reserve-Data $instance
$environment = New-Environment $homePath $instance (New-Port)
foreach ($folder in @('LocalApplicationData', 'ApplicationData')) {
    $profile = Join-Path ([Environment]::GetFolderPath($folder)) "com.domios.dev.$instance"
    Assert-Proof (!(Test-Path -LiteralPath $profile)) 'Refuse an existing native UI profile'
    [IO.Directory]::CreateDirectory($profile) | Out-Null
    $ownedNativeData.Add($profile)
}
Write-Evidence 'owned-native-data.json' $ownedNativeData.ToArray()
$tree = Start-Owned $Client @() $environment 8192
try {
    Assert-Proof ($tree.JobFlags -eq 8192 -and $tree.InJob($tree.Pid)) 'Native client did not enter the unsafe fixture job'
    $remaining = [Math]::Min(35000, 300000 - $globalClock.ElapsedMilliseconds)
    Assert-Proof ($remaining -gt 0) 'Fixture global deadline before native UI observation'
    $text = [JobFailureUi]::Wait($tree.Pid, $Cause, (Join-Path $Out 'native-startup.png'), $remaining)
    Assert-Proof ($tree.Alive($tree.Pid)) 'Native failure window process exited'
    $logs = @(Get-ChildItem -LiteralPath $data -Recurse -File -Filter 'client-startup.log')
    Assert-Proof ($logs.Count -eq 1) 'Expected one startup log in the exact isolated home'
    $log = [IO.File]::ReadAllText($logs[0].FullName)
    Assert-Proof ($log.Contains($Cause)) 'Client startup log lost the causal job refusal'
    [IO.File]::WriteAllText((Join-Path $Out 'client-startup.log'), $log)
    Write-Evidence 'native-ui.json' @{ pid = $tree.Pid; identity = $tree.Identity($tree.Pid);
        flags = $tree.JobFlags; visible_cause = $Cause; text = $text; screenshot = 'native-startup.png';
        startup_log = 'client-startup.log'; processes = $tree.ProcessEvidence }
} finally {
    Stop-Owned $tree
}
Write-Evidence 'native-ui-cleanup.json' @{ all_pinned_processes_exited = $true; exits = $tree.ExitEvidence }
