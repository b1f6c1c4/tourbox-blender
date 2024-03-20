from functools import cache, partial, reduce
from math import inf, pow
import re
import time
from typing import Literal
import bpy

from bpy.types import Context
from tourbox_addon.brush import (
    ActiveBrush,
    get_active_brush,
    get_paint,
    set_active_brush,
)
from tourbox_addon.data import modify_store
from tourbox_addon.util import default_context

import xdotool
import os

DialPrefix = Literal["MouseWheel", "TallDial", "FlatWheel"]
BRUSH_SET_BUTTONS = (
    "DpadLeft",
    "DpadRight",
    "DpadUp",
    "DpadDown",
    "BottomRightClickerLeft",
    "BottomRightClickerRight",
    "SideThumb",
    "LongBarButton",
)
WHEEL_DIRS = ("Right", "Down", "Up", "Left")


_ModeProfile__button_states = dict()
_BrushModeProfile__timeout = inf
TIMEOUT = 1.0


@default_context
def bind_active_brush_button(ctx: Context, button: str):
    brush = get_active_brush()
    with modify_store() as store:
        newbrush = store.overwrite_brush(ctx.mode, button, brush)
    set_active_brush(ctx, newbrush)


class BaseProfile:
    def __init__(self) -> None:
        self.flat_wheel_timestamp = time.clock_gettime_ns(time.CLOCK_MONOTONIC_RAW)
        self.flat_wheel_dir = 0
        self.btns = set()
        self.val0 = None
        self.path = None

    def tall_dial(self, pressed: bool, direction: int):
        val = eval(self.path)
        if self.val0 == 0:
            val += direction * (0.1 if pressed else 1)
        else:
            val += direction * (0.01 if pressed else 0.1) * self.val0
        exec(self.path + ' = ' + str(val))

    def flat_wheel(self, pressed: bool, direction: int):
        if pressed:
            bpy.ops.screen.keyframe_jump(next=(direction == 1))
        else:
            now = time.clock_gettime_ns(time.CLOCK_MONOTONIC_RAW)
            diff = now - self.flat_wheel_timestamp
            if self.flat_wheel_dir == direction:
                self.flat_wheel_timestamp = now
                speed = max(1, min(180e6 / diff, 40))
                delta = int(pow(speed, 1.8))
            else:
                self.flat_wheel_dir = direction
                delta = 1
            bpy.context.scene.frame_set(bpy.context.scene.frame_current + direction * delta)

    def mouse_wheel(self, pressed: bool, direction: int):
        val = eval(self.path)
        val *= 1 - direction * (0.01 if pressed else 0.1)
        exec(self.path + ' = ' + str(val))

    def button_press(self, prefix: str):
        self.btns.add(prefix)

        if prefix == "LogoButtonRight":
            lst = ["SCULPT","POSE","SCULPT_CURVES"]
            if bpy.context.mode not in lst:
                id = -1
            else:
                id = lst.index(bpy.context.mode)
            for tries in range(len(lst)):
                try:
                    bpy.ops.object.mode_set(mode=lst[(id + 1 + tries) % len(lst)])
                    break
                except:
                    pass
        elif prefix == "LogoButtonLeft":
            if bpy.context.mode != "OBJECT":
                bpy.ops.object.mode_set(mode="OBJECT")
            else:
                bpy.ops.object.mode_set(mode="EDIT")
        elif prefix == "SideThumb":
            xdotool.keydown(keysequence=['ctrl'])
        elif prefix == "LongBarButton":
            xdotool.keydown(keysequence=['shift'])
        elif prefix == "BottomRightClickerRight":
            xdotool.keydown(keysequence=['alt'])
        elif prefix == "ButtonNearTallDial":
            xdotool.keydown(keysequence=['grave'])
        elif prefix == "BottomRightClickerLeft":
            xdotool.key(keysequence=['ctrl+alt+shift+c'])
            paste = os.path.dirname(os.path.realpath(__file__)) + '/paste.sh'
            os.system(paste)
            fn = '/tmp/xclip-workaround'
            for i in range(20):
                if not os.path.exists(fn):
                    time.sleep(0.05)
                    continue
                with open(fn, 'r') as f:
                    self.path = f.read()
                print(self.path)
                self.val0 = eval(self.path)
                return


    def button_release(self, prefix: str):
        self.btns.remove(prefix)

        if prefix == "SideThumb":
            xdotool.keyup(keysequence=['ctrl'])
        elif prefix == "LongBarButton":
            xdotool.keyup(keysequence=['shift'])
        elif prefix == "BottomRightClickerRight":
            xdotool.keyup(keysequence=['alt'])
        elif prefix == "ButtonNearTallDial":
            xdotool.keyup(keysequence=['grave'])

    def button_state(self, prefix: str) -> bool:
        return prefix in self.btns


class BrushModeProfile(BaseProfile):
    def tall_dial(self, pressed: bool, direction: int):
        self.brush.size += (2 if pressed else 20) * direction

    def flat_wheel(self, pressed: bool, direction: int):
        self.brush.strength += (0.2 if not pressed else 0.008) * direction
        self.brush.flow += (0.2 if not pressed else 0.008) * direction

    def button_press(self, prefix: str):
        global __timeout
        super().button_press(prefix)
        if prefix == "ButtonNearTallDial":
            self.brush.direction = not self.brush.direction
        elif prefix in BRUSH_SET_BUTTONS:
            __timeout = time.time()
            with modify_store() as store:
                newbrush = store.get_brush(bpy.context.mode, prefix)
                if newbrush is not None:
                    set_active_brush(bpy.context, newbrush)

    def button_release(self, prefix: str):
        global __timeout
        super().button_release(prefix)
        if prefix in BRUSH_SET_BUTTONS:
            if time.time() - __timeout >= TIMEOUT:
                bind_active_brush_button(bpy.context, prefix)
            __timeout = inf

bmp = BrushModeProfile()
mp = BaseProfile()

def get_profile() -> BaseProfile:
    if get_paint() is not None:
        bmp.brush = ActiveBrush(bpy.context)
        return bmp
    return mp


def on_input_event(event: str):
    profile = get_profile()
    if "Press" in event:
        profile.button_press(event.replace("Press", ""))
    elif "Release" in event:
        profile.button_release(event.replace("Release", ""))
    expr = "|".join(WHEEL_DIRS)
    prefix = re.sub(expr, "", event)
    pressed = profile.button_state(prefix)
    direction = 1 if ("Right" in event or "Down" in event) else -1
    if prefix == "TallDial":
        profile.tall_dial(pressed, direction)
    elif prefix == "FlatWheel":
        profile.flat_wheel(pressed, direction)
    elif prefix == "MouseWheel":
        profile.mouse_wheel(pressed, direction)
