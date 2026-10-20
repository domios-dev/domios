param([Parameter(Mandatory=$true)][int]$ShellProcessId,[Parameter(Mandatory=$true)][string]$OutputFile)
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class BaselineConsole {
 [StructLayout(LayoutKind.Sequential)] public struct COORD {public short X,Y;}
 [StructLayout(LayoutKind.Sequential)] public struct RECT {public short Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct INFO {public COORD Size,Cursor; public short Attributes; public RECT Window; public COORD Maximum;}
 [DllImport("kernel32.dll")] static extern bool FreeConsole();
 [DllImport("kernel32.dll")] static extern bool AttachConsole(uint pid);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern IntPtr CreateFile(string path,uint access,uint share,IntPtr security,uint creation,uint flags,IntPtr template);
 [DllImport("kernel32.dll")] static extern bool GetConsoleScreenBufferInfo(IntPtr handle,out INFO info);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)] static extern bool ReadConsoleOutputCharacter(IntPtr handle,StringBuilder text,uint length,COORD start,out uint read);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 public static string Read(uint pid) {
  FreeConsole(); if(!AttachConsole(pid)) throw new Exception("AttachConsole failed "+Marshal.GetLastWin32Error());
  var handle=CreateFile("CONOUT$",0x80000000,3,IntPtr.Zero,3,0,IntPtr.Zero);
  try {
   INFO info; if(!GetConsoleScreenBufferInfo(handle,out info))throw new Exception("No console buffer");
   var text=new StringBuilder(); var line=new StringBuilder(info.Size.X);
   for(short y=info.Window.Top;y<=info.Window.Bottom;y++){
    uint read; line.Clear(); ReadConsoleOutputCharacter(handle,line,(uint)info.Size.X,new COORD{X=0,Y=y},out read);
    text.AppendLine(line.ToString().Substring(0,(int)read).TrimEnd());
   }
   return text.ToString();
  } finally {CloseHandle(handle);FreeConsole();}
 }
}
"@
$taskText=[BaselineConsole]::Read([uint32]$ShellProcessId)
[IO.File]::WriteAllText($OutputFile,$taskText)
