from abc import ABC, abstractmethod

class TransferEvents(ABC):
    @abstractmethod
    def on_task_added(self, task_id, filename, side="SEND"): pass
    @abstractmethod
    def on_progress(self, task_id, current, total): pass
    @abstractmethod
    def on_status_change(self, task_id, status, message=""): pass
    @abstractmethod
    def on_error(self, task_id, error_msg): pass