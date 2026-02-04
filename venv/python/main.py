import sys
import asyncio
import argparse
import logging
import os
import threading
import ctypes

from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.align import Align
from rich import box
from prompt_toolkit import PromptSession
from prompt_toolkit.history import InMemoryHistory
from prompt_toolkit.formatted_text import HTML
from prompt_toolkit.patch_stdout import patch_stdout

from droptea_core import DropTeaEngine, send_handshake
from logger_config import setup_logging
from cli_adapter import CLITransferUI
from transfer_manager import AsyncTransferManager
from network_service import AsyncReceiver

def enable_windows_virtual_terminal():
    if sys.platform == "win32":
        try:
            kernel32 = ctypes.windll.kernel32
            hOut = kernel32.GetStdHandle(-11)
            out_mode = ctypes.c_ulong()
            kernel32.GetConsoleMode(hOut, ctypes.byref(out_mode))
            new_mode = out_mode.value | 0x0004
            kernel32.SetConsoleMode(hOut, new_mode)
        except Exception: pass

enable_windows_virtual_terminal()

logger = logging.getLogger("Main")
console = Console()

active_peers = {}
request_event = threading.Event()
ui_cancel_event = asyncio.Event() 
user_decision = False
pending_request = {} 
global_session = None 
startup_future = None

# ✅ เพิ่มฟังก์ชัน Helper ที่ขาดหายไปกลับเข้ามา (แก้ NameError)
def rust_cert_callback(*args) -> bool:
    # Callback นี้จะถูกเรียกถ้า Rust ต้องการให้ User ยืนยัน Certificate
    return True

def get_file_icon(filename):
    ext = filename.split('.')[-1].lower() if '.' in filename else ""
    if ext in ['jpg', 'jpeg', 'png', 'gif', 'webp']: return "🖼️"
    if ext in ['mp4', 'mov', 'avi', 'mkv']: return "🎬"
    if ext in ['mp3', 'wav', 'flac']: return "🎵"
    if ext in ['zip', 'rar', '7z', 'tar', 'gz']: return "📦"
    if ext in ['pdf', 'doc', 'docx', 'txt']: return "📄"
    return "📁"

def get_os_icon(os_name):
    os_name = os_name.lower()
    if "win" in os_name: return "🪟 Windows"
    if "mac" in os_name or "darwin" in os_name: return "🍎 macOS"
    if "linux" in os_name: return "🐧 Linux"
    return "💻 Device"

def print_file_request(console, filename, filesize, sender_name, sender_device):
    size_str = ""
    if filesize < 1024: size_str = f"{filesize} B"
    elif filesize < 1024**2: size_str = f"{filesize/1024:.1f} KB"
    elif filesize < 1024**3: size_str = f"{filesize/(1024**2):.2f} MB"
    else: size_str = f"{filesize/(1024**3):.2f} GB"

    file_icon = get_file_icon(filename)
    os_icon = get_os_icon(sender_device)

    grid = Table.grid(expand=True, padding=(0, 2))
    grid.add_column(justify="right", style="dim", width=12)
    grid.add_column(justify="left")
    grid.add_row("From:", f"[bold cyan]{sender_name}[/]")
    grid.add_row("Device:", f"[magenta]{os_icon}[/]")
    grid.add_row("File:", f"{file_icon}  [bold yellow]{filename}[/]")
    grid.add_row("Size:", f"[green]{size_str}[/]")

    console.print(Panel(
        Align.center(grid), 
        title="[bold green]📨 Incoming Request[/]", 
        border_style="green",
        box=box.ROUNDED,
        padding=(1, 4),
        subtitle="[bold white]Type 'y' to accept or 'n' to decline[/]"
    ))

class RustDiscoveryAdapter:
    def __init__(self): self.peers = active_peers

class RustEventHandler:
    def __init__(self, receiver, ui, loop):
        self.receiver = receiver; self.ui = ui; self.loop = loop

    def handle_incoming_request(self, task_id, filename, filesize, sender_name, sender_device):
        global user_decision
        
        try: fsize_int = int(filesize)
        except: fsize_int = 0

        pending_request.clear()
        self.loop.call_soon_threadsafe(ui_cancel_event.clear)

        pending_request.update({
            'type': 'file', 'task_id': task_id, 'filename': filename, 
            'filesize': fsize_int, 'sender_name': sender_name, 'sender_device': sender_device
        })
        request_event.clear()
        user_decision = False
        
        if global_session and global_session.app.is_running: 
            self.loop.call_soon_threadsafe(global_session.app.exit)
        
        is_set = request_event.wait(timeout=60)

        if not is_set:
            user_decision = False
            self.loop.call_soon_threadsafe(ui_cancel_event.set)
            pending_request.clear()
            if global_session and global_session.app.is_running: 
                self.loop.call_soon_threadsafe(global_session.app.exit)
            return False

        return user_decision

    def __call__(self, *args):
        global user_decision, startup_future
        if not args: return
        event_type = args[0]

        if event_type == "SERVER_STARTED":
            if startup_future and not startup_future.done():
                self.loop.call_soon_threadsafe(startup_future.set_result, True)
            self.receiver._rust_callback(event_type, args[1], args[2])
            return
        elif event_type == "ERROR" and args[1] == "system" and "Startup failed" in args[2]:
            if startup_future and not startup_future.done():
                self.loop.call_soon_threadsafe(startup_future.set_result, False)
            return

        if len(args) >= 3:
            task_id, data = args[1], args[2]
            
            if event_type == "Incoming" and data.startswith("[[REQUEST]]"):
                try:
                    content = data.replace("[[REQUEST]]|", "")
                    parts = content.split('|')
                    if len(parts) >= 4:
                        self.handle_incoming_request(task_id, parts[0], parts[1], parts[2], parts[3])
                        return 
                except Exception as e:
                    logger.error(f"Failed to parse incoming request: {e}")

            if event_type == "Incoming" and "[[START]]" in data:
                 if pending_request and pending_request.get('task_id') == task_id:
                     request_event.set()
                     pending_request.clear()

            if event_type == "PEER_FOUND":
                try: 
                    parts = data.split('|')
                    if len(parts) >= 3:
                        name, ip, port = parts[0], parts[1], int(parts[2])
                        ssid = parts[3] if len(parts) > 3 else "?"
                        transport = parts[4] if len(parts) > 4 else "LAN"
                        active_peers[task_id] = {'name': name, 'ip': ip, 'port': port, 'ssid': ssid, 'transport': transport}
                except: pass

            elif event_type == "PEER_LOST":
                if task_id in active_peers: del active_peers[task_id]
            
            self.receiver._rust_callback(event_type, task_id, data)

async def input_loop(transfer_mgr, ui, engine, config_name):
    global global_session, user_decision
    ui.print_banner()
    ui.print_system(f"Identity: [bold green]{engine.get_my_name()}[/]")
    ui.print_system(f"Config: [bold cyan]{config_name}[/]") 
    
    session = PromptSession(history=InMemoryHistory())
    global_session = session 
    
    while True:
        try:
            with patch_stdout():
                if not pending_request: 
                    cmd = await session.prompt_async(HTML(f"<b><green>DropTea</green></b> ({engine.get_my_name()}) > "))
                else:
                    cmd = "" 

            if pending_request:
                req_type = pending_request.get('type')
                msg = ""
                
                if req_type == 'file':
                    print_file_request(
                        ui.console, 
                        pending_request.get('filename'), 
                        pending_request.get('filesize', 0), 
                        pending_request.get('sender_name', 'Unknown'),
                        pending_request.get('sender_device', 'Unknown')
                    )
                    msg = "👉 Accept File? (y/n): "
                
                try:
                    ans = await session.prompt_async(HTML(f"<b><yellow>{msg}</yellow></b>"))
                    if ui_cancel_event.is_set():
                        pending_request.clear()
                        continue
                        
                    decision = ans.strip().lower() in ('y', 'yes', '')
                    
                    if req_type == 'file':
                        task_id = pending_request.get('task_id')
                        engine.resolve_request(task_id, decision)

                except (EOFError, KeyboardInterrupt):
                    pending_request.clear()
                
                pending_request.clear()
                request_event.set()
                continue

            parts = cmd.strip().split()
            if not parts: continue
            
            if parts[0] == "list":
                if not active_peers: ui.console.print("[dim]No peers found yet...[/]")
                else:
                    for i, (pid, info) in enumerate(active_peers.items()):
                        ui.console.print(f"  [bold green]{i}[/] : {info['name']} [dim]({info['ip']}:{info['port']})[/]")
            
            elif parts[0] == "connect" and len(parts) >= 2:
                target_mac = parts[1]
                ui.console.print(f"[dim]👉 Connecting to {target_mac}...[/]")
                try:
                    await send_handshake(target_mac)
                except Exception as e:
                    ui.console.print(f"[red]❌ Connection Failed: {e}[/]")

            elif parts[0] == "drop":
                if len(parts) < 3:
                    ui.console.print("[yellow]Usage: drop <index> <file_path>[/]")
                    continue
                try:
                    idx = int(parts[1])
                    path = " ".join(parts[2:]).strip("'\"")
                    
                    peers_list = list(active_peers.keys()) 
                    if 0 <= idx < len(peers_list):
                        target_peer_id = peers_list[idx]
                        if os.path.exists(path):
                            await transfer_mgr.add_task(path, target_peer_id)
                            target_name = active_peers[target_peer_id]['name']
                            ui.console.print(f"[green]🚀 Sending '{os.path.basename(path)}' to {target_name}...[/]")
                        else:
                            ui.console.print(f"[red]❌ File not found: {path}[/]")
                    else:
                        ui.console.print("[red]❌ Invalid peer index (check 'list')[/]")
                except ValueError:
                    ui.console.print("[red]❌ Index must be a number[/]")
                except Exception as e:
                    ui.console.print(f"[red]❌ Error: {e}[/]")
            
            elif parts[0] == "exit": break
        except (EOFError, KeyboardInterrupt): break

async def main():
    global startup_future
    args = parse_args()
    
    # ✅ 1. เลือกไฟล์ Config จาก args หรือใช้ default
    config_path = args.config 
    
    if not os.path.exists(config_path):
        console.print(f"[red]❌ Error: Config file '{config_path}' not found.[/]")
        return

    setup_logging(log_filename=f"logs/{os.path.basename(config_path)}.jsonl", debug_mode=args.verbose)
    
    # 🔥 Updated: Pass args.verbose as dev_mode to enable Engineering Report
    ui = CLITransferUI(console=console, dev_mode=args.verbose)
    
    main_loop = asyncio.get_running_loop()
    shared_engine = DropTeaEngine()
    
    # ✅ rust_cert_callback ถูกประกาศแล้วก่อนหน้านี้ จึงไม่ Error
    transfer_mgr = AsyncTransferManager(None, ui, engine=shared_engine, loop=main_loop, cert_verifier=rust_cert_callback) 
    receiver = AsyncReceiver(ui, engine=shared_engine, loop=main_loop)
    transfer_mgr.discovery = RustDiscoveryAdapter()

    startup_future = main_loop.create_future()
    ui.console.print(f"[dim]Starting Rust Core with [bold cyan]{config_path}[/]...[/]")
    
    try:
        # ✅ 2. ส่ง Path ของ Config ที่เลือกลงไปให้ Rust
        shared_engine.start_server(
            config_path, 
            RustEventHandler(receiver, ui, main_loop)
        )
        await startup_future 
    except Exception as e:
        ui.console.print(f"[red]❌ Failed to start Core: {e}[/]")
        return
    
    worker = asyncio.create_task(transfer_mgr.start_worker())
    try: await input_loop(transfer_mgr, ui, shared_engine, config_path)
    finally: worker.cancel()

def parse_args():
    parser = argparse.ArgumentParser(description="DropTea P2P File Transfer")
    parser.add_argument("-v", "--verbose", action="store_true", help="Enable verbose logging")
    
    # ✅ 3. เพิ่ม Argument --config
    parser.add_argument("-c", "--config", type=str, default="config/config.toml", help="Path to configuration file (default: config.toml)")
    
    return parser.parse_args()

if __name__ == "__main__":
    if sys.platform == 'win32': asyncio.set_event_loop_policy(asyncio.WindowsSelectorEventLoopPolicy())
    try: asyncio.run(main())
    except KeyboardInterrupt: pass