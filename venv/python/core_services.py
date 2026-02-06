# core_services.py
import asyncio
import logging
import os
from dataclasses import dataclass, field
from typing import Optional
from droptea_core import DropTeaEngine
from constants import PROTOCOL_PREFIX_REQUEST, PROTOCOL_PREFIX_START

logger = logging.getLogger("CoreService")

@dataclass
class TransferTask:
    file_path: str
    peer_ip: str
    peer_port: int
    task_id: str
    target_os: Optional[str] = None

class TransferController:
    """Manages outgoing file transfers."""
    def __init__(self, state_manager, ui, engine: DropTeaEngine, device_name: str):
        self.state = state_manager
        self.ui = ui
        self.engine = engine
        self.device_name = device_name
        self.queue = asyncio.Queue()
        self._running = True

    async def queue_file(self, file_path, peer_id):
        peer = self.state.peers.get(peer_id)
        if not peer:
            self.ui.on_error("System", f"Peer ID '{peer_id}' not found in registry.")
            return
        
        task_id = os.path.basename(file_path)
        
        # Simple OS detection based on peer name hint
        detected_os = None
        p_name = peer.get('name', '').lower()
        if "iphone" in p_name or "ipad" in p_name: 
            detected_os = "ios"
        elif "mac" in p_name: 
            detected_os = "macos"

        task = TransferTask(file_path, peer['ip'], peer['port'], task_id, detected_os)
        await self.queue.put(task)
        self.ui.on_task_added(task_id, task_id)

    # Callback Wrapper for Rust
    def _rust_callback(self, *args):
        # Rust sends args: (event_type, task_id, data)
        # We wrap this in a broad try-except to prevent Rust FFI panics
        try:
            if not args: return
            event = args[0]
            
            # Handle certificate verification if needed (Future expansion)
            if event == "ask_verify_certificate":
                return False 

            if len(args) < 3: return
            task_id, data = args[1], args[2]

            if event == "START": 
                self.ui.on_start(task_id, str(data))
            
            elif event == "PROGRESS":
                try:
                    if isinstance(data, str) and "|" in data: 
                        c, t = map(int, data.split("|"))
                    elif isinstance(data, (list, tuple)): 
                        c, t = data
                    else: 
                        return
                    self.ui.on_progress(task_id, c, t)
                except ValueError:
                    # Non-critical: skip malformed progress update
                    pass
            
            elif event == "COMPLETED": 
                self.ui.on_status_change(task_id, "COMPLETED")
            
            elif event == "ERROR": 
                self.ui.on_error(task_id, str(data))
        
        except Exception:
            # Critical: Log traceback but do not let it crash the Rust thread
            logger.exception("Unexpected error in Sender Callback")

    async def run_worker(self):
        logger.info("TransferController worker started.")
        while self._running:
            try:
                task = await self.queue.get()
                self.engine.send_file(
                    task.peer_ip, 
                    task.peer_port, 
                    task.file_path, 
                    task.task_id, 
                    self._rust_callback, # Pass bound method
                    self.device_name,
                    task.target_os
                )
                self.queue.task_done()
            except asyncio.CancelledError:
                break
            except Exception as e:
                logger.error(f"Error processing transfer task: {e}", exc_info=True)

    def stop(self):
        self._running = False

class EventOrchestrator:
    """Handles incoming events from Rust Core and dispatches them to UI/State."""
    def __init__(self, state_manager, ui, loop):
        self.state = state_manager
        self.ui = ui
        self.loop = loop

    def dispatch(self, event_type, task_id, data):
        # Ensure thread safety by scheduling execution on the main asyncio loop
        self.loop.call_soon_threadsafe(self._handle_event, event_type, task_id, data)

    def _handle_event(self, event_type, task_id, data):
        try:
            if event_type == "Incoming":
                str_data = str(data)
                
                # Protocol: Request Packet
                if str_data.startswith(PROTOCOL_PREFIX_REQUEST):
                    try:
                        parts = str_data.replace(PROTOCOL_PREFIX_REQUEST, "").split("|")
                        if len(parts) >= 4:
                            fname, fsize, sender, device = parts[0], parts[1], parts[2], parts[3]
                            self.state.requests.add(task_id, fname, fsize, sender, device)
                            self.ui.handle_incoming_request(task_id, fname, fsize, sender, device)
                        else:
                            logger.warning(f"Malformed incoming request data: {str_data}")
                    except IndexError:
                        logger.error(f"Failed to parse incoming request: {str_data}")
                
                # Protocol: Start Packet
                elif str_data.startswith(PROTOCOL_PREFIX_START):
                    fname = str_data.replace(PROTOCOL_PREFIX_START, "")
                    self.ui.on_start(task_id, fname)

            elif event_type == "PEER_FOUND":
                # Format: Name|IP|Port|SSID|Transport
                try:
                    parts = str(data).split('|')
                    if len(parts) >= 3:
                        name, ip, port = parts[0], parts[1], int(parts[2])
                        transport = parts[4] if len(parts) > 4 else "TCP"
                        self.state.peers.update(task_id, name, ip, port, transport)
                except (ValueError, IndexError):
                    logger.warning(f"Invalid peer data received: {data}")
            
            elif event_type == "PEER_LOST":
                self.state.peers.remove(task_id)

            elif event_type == "PROGRESS":
                try:
                    if isinstance(data, str) and "|" in data: 
                        c, t = map(int, data.split("|"))
                    else: 
                        c, t = data
                    self.ui.on_progress(task_id, c, t)
                except ValueError: 
                    pass
            
            elif event_type == "COMPLETED":
                self.ui.on_status_change(task_id, "COMPLETED")
            
            elif event_type == "ERROR":
                err_msg = str(data)
                if "deadline" in err_msg or "time" in err_msg.lower(): 
                    err_msg = "Timeout / No Response"
                self.ui.on_error(task_id, err_msg)
            
            elif event_type == "REJECTED":
                self.ui.on_reject(task_id, str(data))

            elif event_type == "SERVER_STARTED":
                logger.info(f"Core server started successfully on port {task_id}")

        except Exception:
            # Log full traceback for debugging, but don't crash the loop
            logger.exception(f"Error handling event '{event_type}' for task '{task_id}'")

class RustEventBridge:
    """Callable wrapper to pass to Rust, delegating to Orchestrator."""
    def __init__(self, orchestrator):
        self.orch = orchestrator
    
    def __call__(self, *args):
        # Rust FFI bridge
        if not args: return
        event_type = args[0]
        # Handle variable arguments safely
        task_id = args[1] if len(args) > 1 else ""
        data = args[2] if len(args) > 2 else ""
        self.orch.dispatch(event_type, task_id, data)