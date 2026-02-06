import pytest
from state_manager import PeerRegistry, RequestManager

def test_peer_registry():
    registry = PeerRegistry()
    
    # 1. Test Add
    registry.update("peer1", "MacBook", "192.168.1.5", 8080, "tcp")
    peer = registry.get("peer1")
    assert peer is not None
    assert peer['name'] == "MacBook"
    
    # 2. Test Remove
    registry.remove("peer1")
    assert registry.get("peer1") is None

def test_request_manager():
    req_mgr = RequestManager()
    
    # 1. Test Add Request
    req_mgr.add("task_123", "resume.pdf", 1024, "John", "iPhone")
    req = req_mgr.get("task_123")
    assert req['filename'] == "resume.pdf"
    
    # 2. Test Remove
    req_mgr.remove("task_123")
    assert req_mgr.get("task_123") is None