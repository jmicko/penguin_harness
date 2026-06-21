use crate::native_input::{NativeInputDevice, NativeInputResult};
use crate::types::MouseButton;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct NativeCheck {
    pub input: crate::native_input::NativeInputCheck,
    pub screenshot: crate::native_screenshot::NativeScreenshotCheck,
}

pub struct NativeController {
    input: Option<NativeInputDevice>,
}

impl NativeController {
    pub fn new() -> Self {
        Self { input: None }
    }

    pub fn click_screen(
        &mut self,
        x: i32,
        y: i32,
        button: MouseButton,
    ) -> Result<NativeInputResult> {
        self.input()?.click_screen(x, y, button)
    }

    pub fn double_click_screen(
        &mut self,
        x: i32,
        y: i32,
        button: MouseButton,
    ) -> Result<NativeInputResult> {
        self.input()?.double_click_screen(x, y, button)
    }

    pub fn drag_screen(
        &mut self,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        button: MouseButton,
    ) -> Result<NativeInputResult> {
        self.input()?.drag_screen(x1, y1, x2, y2, button)
    }

    pub fn scroll_screen(&mut self, x: i32, y: i32, amount: i32) -> Result<NativeInputResult> {
        self.input()?.scroll_screen(x, y, amount)
    }

    pub fn type_text(&mut self, text: &str) -> Result<NativeInputResult> {
        self.input()?.type_text(text)
    }

    pub fn press_key(&mut self, key: &str) -> Result<NativeInputResult> {
        self.input()?.press_key(key)
    }

    fn input(&mut self) -> Result<&mut NativeInputDevice> {
        if self.input.is_none() {
            self.input = Some(NativeInputDevice::create()?);
        }
        Ok(self.input.as_mut().expect("input was just initialized"))
    }
}

pub fn check() -> NativeCheck {
    NativeCheck {
        input: crate::native_input::check(),
        screenshot: crate::native_screenshot::check(),
    }
}
