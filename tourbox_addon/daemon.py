from functools import partial
import socket
from threading import Thread

import bpy

from tourbox_addon.events import on_input_event


sock = None


def start_daemon():
    global sock
    if sock is not None:
        return
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("0.0.0.0", 21404))
    t = Thread(target=thread_entry, args=(sock,))
    t.start()


def stop_daemon():
    global sock
    if sock is None:
        return
    sock.close()
    sock = None


def thread_entry(sock):
    while True:
        data, addr = sock.recvfrom(1024)
        data = data.decode()
        if data != "Unknown" and data.strip():
            # Hack to get back to a "safe" blender thread, hopefully. But nothing is certain
            bpy.app.timers.register(partial(on_input_event, data), first_interval=0)
