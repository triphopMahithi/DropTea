# main.py
import sys
import asyncio
import argparse
import logging
import os
import ctypes
from rich.console import Console

from droptea_core import DropTeaEngine
from config_manager import Settings
from state_manager import AppState
from core_services import TransferController, EventOrchestrator, RustEventBridge
from ui_presenter import TerminalPresenter
from cli_shell import CommandShell
from logger_config import setup_logging

def enable_windows_terminal():
    """Enable ANSI escape sequences for Windows 10/11 terminals."""
    if sys.platform == "win32":
        try:
            kernel32 = ctypes.windll.kernel32
            h_out = kernel32.GetStdHandle(-11)
            out_mode = ctypes.c_ulong()
            kernel32.GetConsoleMode(h_out, ctypes.byref(out_mode))
            new_mode = out_mode.value | 0x0004
            kernel32.SetConsoleMode(h_out, new_mode)
        except OSError:
            pass

def main():
    enable_windows_terminal()
    
    parser = argparse.ArgumentParser(description="DropTea P2P File Transfer Node")
    parser.add_argument("-v", "--verbose", action="store_true", help="Enable verbose debug logging")
    parser.add_argument("-c", "--config", default="config/config.toml", help="Path to configuration file")
    args = parser.parse_args()

    # 1. Initialize Settings
    if not os.path.exists(args.config):
        print(f"[Fatal Error] Configuration file not found: {args.config}")
        sys.exit(1)

    try:
        settings = Settings(args.config)
    except Exception as e:
        print(f"[Fatal Error] Failed to load settings: {e}")
        sys.exit(1)

    setup_logging(
        settings.logging.file_path, 
        args.verbose, 
        settings.logging.max_size_mb, 
        settings.logging.backup_count
    )
    
    logger = logging.getLogger("Main")

    # 2. Initialize Core & State
    try:
        engine = DropTeaEngine()
        state = AppState()
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
    except Exception as e:
        logger.critical(f"Failed to initialize application core: {e}", exc_info=True)
        sys.exit(1)

    # 3. Initialize UI & Services
    ui = TerminalPresenter(Console(), dev_mode=args.verbose)
    orchestrator = EventOrchestrator(state, ui, loop)
    controller = TransferController(state, ui, engine, engine.get_my_name())
    
    # 4. Start Server
    bridge = RustEventBridge(orchestrator)
    print(f"[System] Starting Core with {args.config}...")
    
    try:
        engine.start_server(args.config, bridge)
    except Exception as e:
        logger.critical(f"Failed to bind/start server: {e}", exc_info=True)
        print(f"[Fatal Error] Failed to start core: {e}")
        sys.exit(1)

    # 5. Hot Reload Handler with Retry Logic (Fixes OS Error 10048 / Address in use)
    async def reload_server():
        max_retries = 5
        retry_delay = 1.0
        
        for i in range(max_retries):
            try:
                # Re-create bridge with fresh orchestrator state if needed
                # (Engine handles internal config reloading)
                engine.start_server(args.config, bridge)
                ui.console.print("[green bold]✔ Server Reloaded Successfully![/]")
                return
            
            except OSError as e:
                # Handle 'Address already in use' (Windows Error 10048 or Unix EADDRINUSE)
                # This often happens when restarting TCP listeners quickly.
                error_code = getattr(e, 'errno', None) or getattr(e, 'winerror', None)
                is_port_busy = error_code == 10048 or "Address already in use" in str(e)
                
                if is_port_busy and i < max_retries - 1:
                    ui.console.print(f"[yellow]⏳ Port is busy, waiting to release... ({i+1}/{max_retries})[/]")
                    await asyncio.sleep(retry_delay)
                    continue
                
                logger.error(f"Reload failed (OSError): {e}", exc_info=True)
                ui.console.print(f"[red]❌ Reload Failed (Network Error): {e}[/]")
                break
                
            except Exception as e:
                logger.exception("Unexpected error during server reload")
                ui.console.print(f"[red]❌ Reload Failed: {e}[/]")
                break

    # 6. Launch Shell
    shell = CommandShell(controller, ui, engine, state, args.config, reload_server)
    
    # Start the worker task for processing the send queue
    worker = loop.create_task(controller.run_worker())
    
    try:
        loop.run_until_complete(shell.run())
    except KeyboardInterrupt:
        logger.info("User interrupted (Ctrl+C). Shutting down.")
    except Exception as e:
        logger.critical(f"Unhandled exception in main loop: {e}", exc_info=True)
        print(f"[Fatal Error] Application crashed: {e}")
    finally:
        worker.cancel()
        print("\n[System] Shutdown complete.")

if __name__ == "__main__":
    main()