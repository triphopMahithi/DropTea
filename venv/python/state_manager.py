# state_manager.py
import logging

logger = logging.getLogger("StateManager")

class PeerRegistry:
    """Manages active peers discovered on the network."""
    def __init__(self):
        self._peers = {}

    def update(self, peer_id, name, ip, port, transport):
        # Only update if changed or new
        if peer_id not in self._peers:
            logger.info(f"New Peer: {name} ({ip}:{port})")
        
        self._peers[peer_id] = {
            'name': name,
            'ip': ip,
            'port': port,
            'transport': transport
        }

    def remove(self, peer_id):
        if peer_id in self._peers:
            logger.info(f"Peer Lost: {self._peers[peer_id]['name']}")
            del self._peers[peer_id]

    def get(self, peer_id):
        return self._peers.get(peer_id)

    def get_all(self):
        return self._peers.copy()

class RequestManager:
    """Manages pending incoming file transfer requests."""
    def __init__(self):
        self._requests = {}

    def add(self, task_id, filename, filesize, sender_name, sender_device):
        self._requests[task_id] = {
            'filename': filename,
            'filesize': filesize,
            'sender_name': sender_name,
            'sender_device': sender_device
        }

    def remove(self, task_id):
        if task_id in self._requests:
            del self._requests[task_id]

    def get_all(self):
        return self._requests.copy()
    
    def get(self, task_id):
        return self._requests.get(task_id)

class AppState:
    """Single source of truth for application state."""
    def __init__(self):
        self.peers = PeerRegistry()
        self.requests = RequestManager()