import sys
import os
import time
import threading
import statistics
import psutil
from rich.console import Console
from rich.panel import Panel
from rich import box
from events import TransferEvents
from utility import format_bytes

class ResourceMonitor:
    """Monitors system resources (Background Thread)"""
    def __init__(self, pid):
        self.process = psutil.Process(pid)
        self.running = False
        self.samples = {'cpu': [], 'memory': []}
        self.thread = None

    def start(self):
        self.running = True
        self.thread = threading.Thread(target=self._monitor_loop, daemon=True)
        self.thread.start()

    def stop(self):
        self.running = False
        if self.thread and self.thread.is_alive():
            self.thread.join(timeout=1.0)

    def _monitor_loop(self):
        try: self.process.cpu_percent()
        except: pass
        while self.running:
            try:
                cpu = self.process.cpu_percent(interval=None)
                mem = self.process.memory_info().rss
                self.samples['cpu'].append(cpu)
                self.samples['memory'].append(mem)
                time.sleep(0.5)
            except (psutil.NoSuchProcess, psutil.AccessDenied): break

    def get_stats(self):
        if not self.samples['cpu']: return None
        return {
            'avg_cpu': statistics.mean(self.samples['cpu']),
            'avg_mem': statistics.mean(self.samples['memory'])
        }

class TerminalPresenter(TransferEvents):
    """
    Plain Text UI Presenter
    เน้นความเรียบง่าย ไม่มีสีรกตา ยกเว้น Banner
    """
    def __init__(self, console=None, dev_mode=False):
        # ✅ ใช้ Console ของ Rich แค่ตอนจำเป็น (Banner)
        self.console = console if console else Console()
        self.dev_mode = dev_mode
        self.task_meta = {} 
        self.monitor = None

    # --- 1. Banner (คงความสวยงามไว้ตามขอ) ---
    def print_banner(self):
        art = """
      ( (
       ) )
    ........
    |      |]  [bold green]DropTea[/]
    \\      /   [dim]Rust Core v1.1[/]
     `----' 
    """
        self.console.print(Panel(art, border_style="green", expand=False))

    def print_system(self, msg):
        # ใช้ Rich print เพื่อให้สี system message นิดหน่อยพองาม
        self.console.print(f"[bold cyan][System][/] {msg}")

    # --- 2. Plain Text Operations (ไม่มีสี ไม่เพี้ยน) ---
    
    def handle_incoming_request(self, task_id, filename, filesize, sender_name, sender_device):
        print(f"\n>> INCOMING REQUEST <<")
        print(f"   From:   {sender_name} ({sender_device})")
        print(f"   File:   {filename}")
        print(f"   Size:   {format_bytes(int(filesize))}")
        print(f"   Action: Type 'accept' to receive or 'reject' to decline.\n")

    def on_task_added(self, task_id, filename, side="SEND"):
        if side == "SEND":
            print(f"[Queue] Added: {os.path.basename(filename)}")

    def on_start(self, task_id, filename):
        short_name = os.path.basename(filename)
        print(f"--> Starting: {short_name}...")
        
        if self.dev_mode and self.monitor is None:
            self.monitor = ResourceMonitor(os.getpid())
            self.monitor.start()

        self.task_meta[task_id] = {
            'start_time': time.time(),
            'filename': short_name,
            'total': 0,
            'last_update': time.time(),
            'last_bytes': 0,
            'speed_samples': []
        }

    def on_progress(self, task_id, current, total):
        meta = self.task_meta.get(task_id)
        if not meta: return

        # คำนวณความเร็ว
        now = time.time()
        dt = now - meta['last_update']
        if dt > 0.5: # อัปเดตทุก 0.5 วิ พอ
            db = current - meta['last_bytes']
            speed = db / dt if dt > 0 else 0
            meta['speed_samples'].append(speed)
            meta['last_update'] = now
            meta['last_bytes'] = current
            meta['total'] = total # อัปเดต total ล่าสุด
            
            # Plain Text Progress Bar
            percent = (current / total) * 100 if total > 0 else 0
            speed_str = format_bytes(speed) + "/s"
            
            # \r เพื่อเขียนทับบรรทัดเดิม
            sys.stdout.write(f"\r   Progress: {percent:.1f}% | {format_bytes(current)} / {format_bytes(total)} | {speed_str}   ")
            sys.stdout.flush()

    def on_status_change(self, task_id, status, message=""):
        if status == "COMPLETED":
            sys.stdout.write("\n") # ขึ้นบรรทัดใหม่จาก Progress bar
            
            meta = self.task_meta.pop(task_id, None)
            if not self.task_meta and self.monitor: self.monitor.stop()

            if meta:
                total_time = time.time() - meta['start_time']
                avg_speed = meta['total'] / total_time if total_time > 0 else 0
                
                print(f"✔ Completed: {meta['filename']}")
                print(f"   Time:  {total_time:.2f}s")
                print(f"   Avg Speed: {format_bytes(avg_speed)}/s")
                
                if self.dev_mode and self.monitor:
                    stats = self.monitor.get_stats()
                    if stats:
                        print(f"   [Dev] CPU: {stats['avg_cpu']:.1f}% | Mem: {format_bytes(stats['avg_mem'])}")
            else:
                print(f"✔ Completed task: {task_id}")

        elif status == "FAILED":
            sys.stdout.write("\n")
            print(f"✘ Failed: {task_id} - {message}")
            if task_id in self.task_meta: del self.task_meta[task_id]

    def on_error(self, task_id, error_msg):
        sys.stdout.write("\n")
        print(f"! Error ({task_id}): {error_msg}")
        if task_id in self.task_meta: del self.task_meta[task_id]

    def on_reject(self, task_id, reason):
        sys.stdout.write("\n")
        print(f"🚫 Rejected ({task_id}): {reason}")
        if task_id in self.task_meta: del self.task_meta[task_id]