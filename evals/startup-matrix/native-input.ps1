param([Parameter(Mandatory=$true)][int]$ShellProcessId,[Parameter(Mandatory=$true)][string]$InputFile,[Parameter(Mandatory=$true)][string]$ReceiptFile)
Add-Type @"
using System;using System.Runtime.InteropServices;
public static class BaselineInput {
 [StructLayout(LayoutKind.Explicit,CharSet=CharSet.Unicode,Size=20)]
 public struct Record {
  [FieldOffset(0)]public ushort EventType;
  [FieldOffset(4)]public int Down;
  [FieldOffset(8)]public ushort Repeat;
  [FieldOffset(10)]public ushort Key;
  [FieldOffset(14)]public char Character;
 }
 [DllImport("kernel32.dll")]static extern bool FreeConsole();
 [DllImport("kernel32.dll")]static extern bool AttachConsole(uint pid);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)]static extern IntPtr CreateFile(string path,uint access,uint share,IntPtr security,uint creation,uint flags,IntPtr template);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode)]static extern bool WriteConsoleInput(IntPtr handle,Record[] records,uint length,out uint written);
 [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr handle);
 public static uint Send(uint pid,string text){
  FreeConsole();if(!AttachConsole(pid))throw new Exception("AttachConsole failed");
  var handle=CreateFile("CONIN$",0x40000000,3,IntPtr.Zero,3,0,IntPtr.Zero);
  try{
   var records=new Record[(text.Length+1)*2];int i=0;
   foreach(char c in text+"\r"){
    records[i++]=new Record{EventType=1,Down=1,Repeat=1,Key=(ushort)(c=='\r'?13:0),Character=c};
    records[i++]=new Record{EventType=1,Down=0,Repeat=1,Key=(ushort)(c=='\r'?13:0),Character=c};
   }
   uint written;if(!WriteConsoleInput(handle,records,(uint)records.Length,out written))throw new Exception("Console input write failed");
   return written;
  }finally{CloseHandle(handle);FreeConsole();}
 }
}
"@
$taskText=[IO.File]::ReadAllText($InputFile).TrimEnd()
$taskStart=[DateTime]::UtcNow.ToString('o')
$taskWatch=[Diagnostics.Stopwatch]::StartNew()
$taskWritten=[BaselineInput]::Send([uint32]$ShellProcessId,$taskText)
$taskReceipt=@{utc=$taskStart;shell_pid=$ShellProcessId;input_file=$InputFile;input_events_written=$taskWritten;write_ms=$taskWatch.Elapsed.TotalMilliseconds;delivery='OS console input accepted; provider processing unconfirmed until own reply'}
$taskReceipt | ConvertTo-Json | Set-Content -LiteralPath $ReceiptFile -Encoding UTF8
