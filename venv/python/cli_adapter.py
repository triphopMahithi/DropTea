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

from utility import draw_ascii_bar, format_bytes

class ResourceMonitor:
    """Background thread to monitor CPU & RAM usage specific to this process."""
    def __init__(self, pid):
        self.process = psutil.Process(pid)
        self.running = False
        self.samples = {
            'cpu': [],
            'memory': [],
        }
        self.thread = None

    def start(self):
        self.running = True
        self.thread = threading.Thread(target=self._monitor_loop, daemon=True)
        self.thread.start()

    def stop(self):
        self.running = False
        if self.thread:
            self.thread.join(timeout=1.0)

    def _monitor_loop(self):
        # Initial call to reset cpu counters
        try: self.process.cpu_percent()
        except: pass
        
        while self.running:
            try:
                # Monitor specifically the current process
                cpu = self.process.cpu_percent(interval=None)
                mem = self.process.memory_info().rss
                
                self.samples['cpu'].append(cpu)
                self.samples['memory'].append(mem)
                
                time.sleep(0.5) # Sampling rate 2Hz
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                break

    def get_stats(self):
        if not self.samples['cpu']: return None
        return {
            'avg_cpu': statistics.mean(self.samples['cpu']),
            'max_cpu': max(self.samples['cpu']),
            'avg_mem': statistics.mean(self.samples['memory']),
            'max_mem': max(self.samples['memory'])
        }

class CLITransferUI(TransferEvents):
    def __init__(self, console=None, dev_mode=False):
        self.console = console if console else Console(force_terminal=True)
        self.dev_mode = dev_mode
        self.task_meta = {} 
        self.monitor = None

    def handle_incoming_request(self, task_id, filename, filesize, sender_name, sender_device):
        short_name = os.path.basename(filename)
        size_str = format_bytes(int(filesize))
        
        # Incoming Request ยังคงใส่กรอบและสีเพื่อให้ User สังเกตเห็นได้ง่าย
        self.console.print(
            Panel(
                f"[bold cyan]📨 Incoming Request[/]\n"
                f"From: [yellow]{sender_name}[/] [dim]({sender_device})[/]\n"
                f"File: [bold green]{short_name}[/] ({size_str})",
                border_style="green",
                box=box.ROUNDED,
                padding=(1, 2),
                expand=False,
                subtitle="[dim]Type 'y' to accept[/]"
            )
        )

    def on_task_added(self, task_id, filename, side="SEND"):
        if side == "SEND":
            short_name = os.path.basename(filename)
            self.console.print(
                Panel(
                    f"[bold cyan]📤 Sending Request[/]  [dim]──▷[/]  [bold yellow]{short_name}[/]",
                    border_style="dim cyan",
                    box=box.ROUNDED,
                    padding=(0, 2),
                    expand=False
                )
            )

    def on_start(self, task_id, filename):
        short_name = os.path.basename(filename)
        # Start Resource Monitor if in Dev Mode
        if self.dev_mode and self.monitor is None:
            self.monitor = ResourceMonitor(os.getpid())
            self.monitor.start()

        self.task_meta[task_id] = {
            'start_time': time.time(),
            'filename': short_name,
            'total': 0,
            'peak_speed': 0.0,
            'speed_samples': [],
            'last_update': time.time(),
            'last_bytes': 0
        }

    def on_progress(self, task_id, current, total):
        meta = self.task_meta.get(task_id)
        if meta:
            now = time.time()
            dt = now - meta['last_update']
            
            # 🔥 Fix: Reduced sampling interval from 0.5s to 0.1s 
            # to capture peak speed on high-speed LAN transfers
            if dt > 0.1:
                db = current - meta['last_bytes']
                inst_speed = db / dt
                meta['speed_samples'].append(inst_speed)
                meta['peak_speed'] = max(meta['peak_speed'], inst_speed)
                meta['last_update'] = now
                meta['last_bytes'] = current

            meta['total'] = total
            elapsed = now - meta['start_time']
            # Average for progress bar
            avg_speed = current / elapsed if elapsed > 0 else 0
            
            draw_ascii_bar(current, total, meta['filename'], speed_bps=avg_speed, elapsed=elapsed)

    def on_status_change(self, task_id, status, message=""):
        if status == "COMPLETED":
            meta = self.task_meta.pop(task_id, None)
            
            # Stop monitor if no tasks left
            if not self.task_meta and self.monitor:
                self.monitor.stop()

            sys.stdout.write("\r" + " "*100 + "\r")
            sys.stdout.flush()

            if meta:
                total_time = time.time() - meta['start_time']
                avg_speed = meta['total'] / total_time if total_time > 0 else 0
                
                if self.dev_mode:
                    sys_stats = self.monitor.get_stats() if self.monitor else None
                    self._print_engineering_report(meta, total_time, avg_speed, sys_stats)
                else:
                    self._print_simple_report(meta, total_time, avg_speed)
            else:
                print(f"\n✔ Completed: {task_id}")
                
        elif status == "FAILED":
            sys.stdout.write("\r" + " "*100 + "\r")
            print(f"\n✘ Failed: {task_id} - {message}")
            if task_id in self.task_meta:
                del self.task_meta[task_id]

    def _print_simple_report(self, meta, total_time, avg_speed):
        print(f"✔ Completed: {meta['filename']}")
        print(f"   ├─ Size:  {format_bytes(meta['total'])}")
        print(f"   ├─ Time:  {total_time:.2f}s")
        print(f"   └─ Speed: {format_bytes(avg_speed)}/s")

    def _print_engineering_report(self, meta, total_time, avg_speed, sys_stats):
        filename = meta['filename']
        peak_speed = meta['peak_speed']

        # 🔥 Fix: If transfer was too fast to get samples, fallback Peak to Average
        if not meta['speed_samples'] or peak_speed == 0:
            peak_speed = avg_speed
        
        # 1. Stability Calculation
        stability_str = "N/A"
        if len(meta['speed_samples']) > 1:
            stdev = statistics.stdev(meta['speed_samples'])
            mean = statistics.mean(meta['speed_samples'])
            if mean > 0:
                cv = stdev / mean
                stability = max(0, (1 - cv) * 100)
                stability_str = f"{stability:.1f}%"

        # 2. Resource & Diagnosis Formatting
        cpu_usage_str = "N/A"
        ram_usage_str = "N/A"
        verdict = "Healthy"

        if sys_stats:
            cpu_val = sys_stats['avg_cpu']
            ram_val_mb = sys_stats['avg_mem'] / (1024 * 1024)
            
            cpu_usage_str = f"{cpu_val:.1f}%"
            ram_usage_str = f"{ram_val_mb:.2f} MB"

            # Diagnosis Logic
            if cpu_val > 90: verdict = "CPU Bound"
            elif cpu_val < 10 and avg_speed < 1_000_000: verdict = "IO/Net Bound"

        # 3. Print Clean Minimalist Report (No Color)
        print(f"✔ Completed: {filename}")
        print(f"   ├─ Size:      {format_bytes(meta['total'])}")
        print(f"   ├─ Time:      {total_time:.4f}s")
        print(f"   ├─ Speed:     {format_bytes(avg_speed)}/s (Peak: {format_bytes(peak_speed)}/s)")
        print(f"   ├─ Stability: {stability_str}")
        print(f"   ├─ Resource:  CPU {cpu_usage_str} | RAM {ram_usage_str}")
        print(f"   └─ Diagnosis: {verdict}")

    def on_error(self, task_id, error_msg):
        sys.stdout.write("\r" + " "*80 + "\r")
        print(f"\n! Error {task_id}: {error_msg}")
        if task_id in self.task_meta:
            del self.task_meta[task_id]

    def on_reject(self, task_id, reason):
        sys.stdout.write("\r" + " "*80 + "\r")
        print(f"\n🚫 Rejected: {task_id} - {reason}")
        if task_id in self.task_meta:
            del self.task_meta[task_id]

    def print_system(self, msg):
        self.console.print(f"[bold cyan]ℹ️  System:[/] {msg}")

    def print_banner(self):
        art = """
      ( (
       ) )
    ........
    |      |]  [bold green]DropTea[/]
    \\      /   [dim]Rust Core v1.0[/]
     `----' 
    """
        self.console.print(Panel(art, border_style="green", expand=False))