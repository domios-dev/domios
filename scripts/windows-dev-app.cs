using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading.Tasks;

public static class DevAppProcess {
    static string Quote(string value) {
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
        var arguments = new string[args.Length];
        for (int i = 0; i < args.Length; i++) arguments[i] = Quote(args[i]);
        using (var child = new Process()) {
            // .NET Framework redirects through synchronous CreatePipe handles, never libuv pipes.
            child.StartInfo = new ProcessStartInfo(app, String.Join(" ", arguments)) {
                UseShellExecute = false, CreateNoWindow = true,
                RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true
            };
            child.StartInfo.EnvironmentVariables.Remove("CODETERM_DEV_APP_ARGS");
            child.Start();
            child.StandardInput.Close();
            var output = Console.OpenStandardOutput();
            var gate = new object();
            var stdout = Task.Factory.StartNew(() => Pump(child.StandardOutput.BaseStream, output, gate), TaskCreationOptions.LongRunning);
            var stderr = Task.Factory.StartNew(() => Pump(child.StandardError.BaseStream, output, gate), TaskCreationOptions.LongRunning);
            child.WaitForExit();
            Task.WaitAll(stdout, stderr);
            return child.ExitCode;
        }
    }
}
