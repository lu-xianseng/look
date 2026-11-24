# 独立验证：本机是否允许 WH_KEYBOARD_LL 全局钩子回调
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using System.Threading;

public class HookProbe {
  delegate IntPtr LowLevelProc(int nCode, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll", SetLastError=true)] static extern IntPtr SetWindowsHookEx(int idHook, LowLevelProc lpfn, IntPtr hMod, uint dwThreadId);
  [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr hhk, int nCode, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll", SetLastError=true)] static extern bool UnhookWindowsHookEx(IntPtr hhk);
  [DllImport("user32.dll")] static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, IntPtr dwExtraInfo);
  [DllImport("kernel32.dll", CharSet=CharSet.Auto)] static extern IntPtr GetModuleHandle(string name);

  static LowLevelProc proc;
  static int count = 0;
  static IntPtr hk = IntPtr.Zero;

  public static int Run() {
    proc = (nCode, wParam, lParam) => {
      if (nCode >= 0) { count++; }
      return CallNextHookEx(IntPtr.Zero, nCode, wParam, lParam);
    };
    hk = SetWindowsHookEx(13, proc, GetModuleHandle(null), 0);
    Console.WriteLine("hk=" + hk + " err=" + Marshal.GetLastWin32Error());

    var inject = new System.Threading.Timer(_ => {
      keybd_event((byte)0x41, 0, 0, IntPtr.Zero);
      Thread.Sleep(30);
      keybd_event((byte)0x41, 0, 2, IntPtr.Zero);
    }, null, 600, 800);

    var quit = new System.Windows.Forms.Timer();
    quit.Interval = 4000;
    quit.Tick += (s, e) => { Application.ExitThread(); };
    quit.Start();

    Application.Run();
    inject.Dispose();
    if (hk != IntPtr.Zero) { UnhookWindowsHookEx(hk); }
    Console.WriteLine("count=" + count);
    return count;
  }
}
'@ -ReferencedAssemblies System.Windows.Forms
Write-Output ("probe result count=" + [HookProbe]::Run())
