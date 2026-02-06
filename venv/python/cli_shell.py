# cli_shell.py
import asyncio
import shlex
import os
import questionary
from typing import Dict, Callable
from rich.console import Console
from rich.table import Table
from rich.panel import Panel
from prompt_toolkit import PromptSession
from prompt_toolkit.completion import NestedCompleter
from prompt_toolkit.patch_stdout import patch_stdout
from prompt_toolkit.formatted_text import HTML
from prompt_toolkit.styles import Style

from droptea_core import send_handshake
from config_manager import ConfigEditor

class CommandShell:
    def __init__(self, controller, ui, engine, state, config_path, reloader):
        self.controller = controller
        self.ui = ui
        self.engine = engine
        self.state = state
        self.config_path = config_path
        self.reloader = reloader
        self.editor = ConfigEditor(config_path)
        self.console = Console()
        self.commands: Dict[str, dict] = {}
        
        self.register_command("send", self.cmd_send, "Send file to a peer")
        self.register_command("list", self.cmd_list, "Show active peers")
        self.register_command("requests", self.cmd_requests, "Show pending requests")
        self.register_command("accept", self.cmd_accept, "Accept request")
        self.register_command("reject", self.cmd_reject, "Reject request")
        self.register_command("connect", self.cmd_connect, "Connect to peer IP")
        self.register_command("config", self.cmd_config, "View/Edit config")
        self.register_command("reload", self.cmd_reload, "Reload server")
        self.register_command("help", self.cmd_help, "Show help")
        self.register_command("clear", self.cmd_clear, "Clear screen")
        self.register_command("exit", self.cmd_exit, "Exit")

    def register_command(self, name: str, func: Callable, help_text: str):
        self.commands[name] = {'func': func, 'help': help_text}

    def _get_completer(self):
        return NestedCompleter.from_nested_dict({
            "send": None, "list": None, "requests": None, "accept": None, "reject": None,
            "connect": None, "clear": None, "help": None, "exit": None, "reload": None,
            "config": {"show": None, "set": {"mode": {"tcp", "quic", "plaintcp"}, "port": None, "save_path": None}}
        })

    async def run(self):
        style = Style.from_dict({'prompt': 'bg:#00aa00 #000000 bold', 'identity': '#00ff00 bold'})
        session = PromptSession(completer=self._get_completer(), style=style)
        
        self.ui.print_banner()
        self.ui.print_system(f"Identity: [bold green]{self.engine.get_my_name()}[/]")
        self.ui.print_system(f"Config: [bold cyan]{self.config_path}[/]")
        
        while True:
            try:
                with patch_stdout():
                    req_count = len(self.state.requests.get_all())
                    notify = f" <style bg='red' fg='white'> {req_count} Requests </style>" if req_count > 0 else ""
                    text = await session.prompt_async(HTML(f"<prompt> DropTea </prompt> (<identity>{self.engine.get_my_name()}</identity>){notify} > "))
                
                if not text.strip(): continue
                parts = shlex.split(text)
                cmd_name = parts[0].lower()
                args = parts[1:]

                if cmd_name in self.commands:
                    await self.commands[cmd_name]['func'](args)
                else:
                    self.console.print(f"[red]❌ Unknown command: '{cmd_name}'[/]")
            except (KeyboardInterrupt, EOFError): break
            except Exception as e: self.console.print(f"[red]❌ Shell Error: {e}[/]")

    # --- Commands ---
    async def cmd_config(self, args):
        if not args or args[0] == "show":
            if os.path.exists(self.config_path):
                try:
                    with open(self.config_path, 'r', encoding='utf-8') as f:
                        self.console.print(Panel(f.read().strip(), title=f"📄 {self.config_path}", border_style="blue"))
                except Exception as e: self.console.print(f"[red]Error: {e}[/]")
            return

        if args[0] == "set" and len(args) >= 3:
            key, value = args[1], args[2]
            section_map = {"mode": "server", "port": "server", "save_path": "storage"}
            section = section_map.get(key)
            if not section: return self.console.print(f"[red]❌ Unknown key: {key}[/]")
            
            if key == "port": 
                try: value = int(value)
                except: return self.console.print(f"[red]❌ Port must be int[/]")

            success, msg = self.editor.update_key(section, key, value)
            if success:
                self.console.print(f"[green]✔ {msg}[/]")
                if self.reloader: await self.reloader()
            else: self.console.print(f"[red]❌ Failed: {msg}[/]")

    async def cmd_reload(self, args):
        if self.reloader: await self.reloader()
        else: self.console.print("[red]❌ Not available[/]")

    async def cmd_help(self, args):
        table = Table(title="Available Commands", box=None)
        table.add_column("Command", style="cyan bold"); table.add_column("Description", style="dim")
        for name, data in self.commands.items(): table.add_row(name, data['help'])
        self.console.print(table)

    async def cmd_list(self, args):
        peers = self.state.peers.get_all()
        if not peers: return self.console.print("[dim]No peers found.[/]")
        table = Table(show_header=True, header_style="bold magenta")
        table.add_column("ID", width=8); table.add_column("Name", style="green"); table.add_column("Address"); table.add_column("Transport", style="yellow")
        for pid, info in peers.items(): table.add_row(pid[:8], info['name'], f"{info['ip']}:{info['port']}", info['transport'])
        self.console.print(table)

    async def cmd_send(self, args):
        peers = self.state.peers.get_all()
        if not peers: return self.console.print("[red]❌ No peers.[/]")
        choices = [f"{info['name']} | {info['ip']}" for info in peers.values()]
        peer_map = {f"{info['name']} | {info['ip']}": pid for pid, info in peers.items()}
        target = await questionary.select("Recipient:", choices=choices, style=questionary.Style([('answer', 'fg:green bold')])).ask_async()
        if not target: return
        path = args[0] if args else os.getcwd()
        f_path = await questionary.path("File:", default=path).ask_async()
        if f_path and await questionary.confirm(f"Send {os.path.basename(f_path)}?").ask_async():
            self.console.print("[green]🚀 Queuing...[/]")
            await self.controller.queue_file(f_path, peer_map[target])

    async def cmd_requests(self, args):
        reqs = self.state.requests.get_all()
        if not reqs: return self.console.print("[dim]No requests.[/]")
        table = Table(title="Pending Requests")
        table.add_column("ID"); table.add_column("File"); table.add_column("From")
        for tid, info in reqs.items(): table.add_row(tid, info['filename'], info['sender_name'])
        self.console.print(table)

    async def cmd_accept(self, args):
        reqs = self.state.requests.get_all()
        tid = args[0] if args else await questionary.select("Accept:", choices=list(reqs.keys())).ask_async()
        if tid in reqs:
            self.engine.resolve_request(tid, True)
            self.state.requests.remove(tid)
            self.console.print(f"[green]✔ Accepted {tid}[/]")
        else: self.console.print(f"[red]❌ Not found[/]")

    async def cmd_reject(self, args):
        reqs = self.state.requests.get_all()
        tid = args[0] if args else await questionary.select("Reject:", choices=list(reqs.keys())).ask_async()
        if tid in reqs:
            self.engine.resolve_request(tid, False)
            self.state.requests.remove(tid)
            self.console.print(f"[red]🚫 Rejected {tid}[/]")
        else: self.console.print(f"[red]❌ Not found[/]")

    async def cmd_connect(self, args):
        if not args: return self.console.print("[yellow]Usage: connect <ip>[/]")
        try: await send_handshake(args[0]); self.console.print("[green]✔ Sent![/]")
        except Exception as e: self.console.print(f"[red]❌ Error: {e}[/]")

    async def cmd_clear(self, args): self.console.clear()
    async def cmd_exit(self, args): self.console.print("[bold red]Bye![/]"); raise EOFError