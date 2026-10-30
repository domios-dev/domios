// Caller: scripts/windows-dev-app.ps1
using System;
using System.Diagnostics;
using System.Collections.Generic;
using System.Threading;
using System.IO;
using System.Text;
using System.Threading.Tasks;

public static class DevAppProcess {
    public static string Quote(string value) {
        var result = new StringBuilder("\"");
        int slashes = 0;
        foreach (char c in value) {
            if (c == '\\') { slashes++; continue; }
            result.Append('\\', c == '"' ? slashes * 2 + 1 : slashes);
            result.Append(c);
            slashes = 0;
        }
        result.Append('\\', slashes * 2);
        return result.Append('"').ToString();
    }

    static void Pump(Stream input, Stream output, object gate) {
        byte[] buffer = new byte[65536];
        int count;
        while ((count = input.Read(buffer, 0, buffer.Length)) != 0) {
            lock (gate) { output.Write(buffer, 0, count); output.Flush(); }
        }
    }

    public static int Run(string app, string[] args) {
        return RunOwned(app, args, Console.OpenStandardOutput(), null, null, CancellationToken.None);
    }

    public static int RunOwned(string app, string[] args, Stream output, string cwd,
        IDictionary<string, string> environment, CancellationToken cancellation) {
        var arguments = new string[args.Length];
        for (int i = 0; i < args.Length; i++) arguments[i] = Quote(args[i]);
        using (var child = new Process()) {
            // .NET Framework redirects through synchronous CreatePipe handles, never libuv pipes.
            child.StartInfo = new ProcessStartInfo(app, String.Join(" ", arguments)) {
                UseShellExecute = false, CreateNoWindow = true,
                RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true
            };
            child.StartInfo.EnvironmentVariables.Remove("CODETERM_DEV_APP_ARGS");
            if (environment != null) {
                var inherited = new List<string>();
                foreach (string key in child.StartInfo.EnvironmentVariables.Keys)
                    if (key.StartsWith("CODETERM_", StringComparison.OrdinalIgnoreCase) ||
                        key.StartsWith("CT_", StringComparison.OrdinalIgnoreCase) ||
                        key == "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") inherited.Add(key);
                foreach (string key in inherited) child.StartInfo.EnvironmentVariables.Remove(key);
                foreach (var item in environment) {
                    if (String.IsNullOrEmpty(item.Value)) child.StartInfo.EnvironmentVariables.Remove(item.Key);
                    else child.StartInfo.EnvironmentVariables[item.Key] = item.Value;
                }
                child.StartInfo.WorkingDirectory = cwd;
            }
            cancellation.ThrowIfCancellationRequested();
            child.Start();
            if (child.Handle == IntPtr.Zero) throw new InvalidOperationException("DEV client process handle unavailable");
            using (cancellation.Register(() => {
                try { if (!child.HasExited) child.Kill(); } catch (InvalidOperationException) { } catch (System.ComponentModel.Win32Exception) { }
            })) {
                child.StandardInput.Close();
                var gate = new object();
                var stdout = Task.Factory.StartNew(() => Pump(child.StandardOutput.BaseStream, output, gate), TaskCreationOptions.LongRunning);
                var stderr = Task.Factory.StartNew(() => Pump(child.StandardError.BaseStream, output, gate), TaskCreationOptions.LongRunning);
                child.WaitForExit();
                Task.WaitAll(stdout, stderr);
                return child.ExitCode;
            }
        }
    }
}
