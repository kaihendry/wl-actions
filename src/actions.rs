use {
    crate::ActionsError,
    std::{
        any::Any,
        collections::HashSet,
        process::{Command, exit},
        rc::Rc,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        thread,
        time::{Duration, Instant},
    },
    wl_proxy::{
        baseline::Baseline,
        fixed::Fixed,
        object::{Object, ObjectCoreApi},
        protocols::{
            ObjectInterface,
            wayland::{
                wl_display::{WlDisplay, WlDisplayHandler},
                wl_keyboard::{WlKeyboard, WlKeyboardHandler, WlKeyboardKeyState},
                wl_pointer::{WlPointer, WlPointerAxis, WlPointerButtonState, WlPointerHandler},
                wl_registry::{WlRegistry, WlRegistryHandler},
                wl_seat::{WlSeat, WlSeatHandler},
                wl_surface::WlSurface,
                wl_touch::{WlTouch, WlTouchHandler},
            },
        },
        simple::{SimpleCommandExt, SimpleProxy},
    },
};

pub struct ActionCounters {
    pub key_presses: AtomicU64,
    pub button_clicks: AtomicU64,
    pub scroll_steps: AtomicU64,
    pub touch_taps: AtomicU64,
}

impl ActionCounters {
    pub fn new() -> Self {
        Self {
            key_presses: AtomicU64::new(0),
            button_clicks: AtomicU64::new(0),
            scroll_steps: AtomicU64::new(0),
            touch_taps: AtomicU64::new(0),
        }
    }

    pub fn total(&self) -> u64 {
        // Only count keys and clicks in total (scroll events are too granular)
        self.key_presses.load(Ordering::Relaxed)
            + self.button_clicks.load(Ordering::Relaxed)
            + self.touch_taps.load(Ordering::Relaxed)
    }
}

pub fn main(quiet: bool, program: Vec<String>) -> Result<(), ActionsError> {
    // Print version info
    let git_hash = option_env!("GIT_HASH").unwrap_or("unknown");
    if !quiet {
        eprintln!("wl-actions ({})", git_hash);
    }

    let server = SimpleProxy::new(Baseline::ALL_OF_THEM).map_err(ActionsError::CreateServer)?;
    let child = Command::new(&program[0])
        .args(&program[1..])
        .with_wayland_display(server.display())
        .spawn()
        .map_err(ActionsError::SpawnChild)?;

    let counters = Arc::new(ActionCounters::new());
    let running = Arc::new(AtomicBool::new(true));
    let start_time = Instant::now();

    // Set up Ctrl+C handler - print summary and exit
    {
        let counters = counters.clone();
        let running = running.clone();
        ctrlc::set_handler(move || {
            running.store(false, Ordering::Relaxed);
            // Clear the live output line
            eprintln!();
            print_summary(&counters, start_time);
            exit(0);
        })
        .expect("Error setting Ctrl-C handler");
    }

    // Spawn display thread if not quiet
    if !quiet {
        let counters_clone = counters.clone();
        let running_clone = running.clone();
        thread::spawn(move || {
            while running_clone.load(Ordering::Relaxed) {
                let keys = counters_clone.key_presses.load(Ordering::Relaxed);
                let clicks = counters_clone.button_clicks.load(Ordering::Relaxed);
                let scrolls = counters_clone.scroll_steps.load(Ordering::Relaxed);
                let touch = counters_clone.touch_taps.load(Ordering::Relaxed);
                let total = counters_clone.total();
                eprint!(
                    "\rKeys: {} | Clicks: {} | Scrolls: {} | Touch: {} | Total: {} (keys+clicks)    ",
                    keys, clicks, scrolls, touch, total
                );
                thread::sleep(Duration::from_millis(100));
            }
        });
    }

    // Run the proxy - this will block until the child exits or server errors
    let counters_for_handler = counters.clone();
    let pressed_keys = Arc::new(Mutex::new(HashSet::new()));
    let pressed_buttons = Arc::new(Mutex::new(HashSet::new()));
    let last_scroll_time = Arc::new(Mutex::new(Instant::now()));
    let pressed_keys_for_handler = pressed_keys.clone();
    let pressed_buttons_for_handler = pressed_buttons.clone();
    let last_scroll_time_for_handler = last_scroll_time.clone();
    let err = server.run(move || WlDisplayHandlerImpl {
        counters: counters_for_handler.clone(),
        pressed_keys: pressed_keys_for_handler.clone(),
        pressed_buttons: pressed_buttons_for_handler.clone(),
        last_scroll_time: last_scroll_time_for_handler.clone(),
    });

    running.store(false, Ordering::Relaxed);

    // Wait for child to exit
    let _ = child.id(); // Just to ensure child is still valid

    // Clear the live output line
    if !quiet {
        eprintln!();
    }

    // Print summary
    print_summary(&counters, start_time);

    Err(ActionsError::ServerFailed(err))
}

fn print_summary(counters: &ActionCounters, start_time: Instant) {
    let duration = start_time.elapsed();
    let keys = counters.key_presses.load(Ordering::Relaxed);
    let clicks = counters.button_clicks.load(Ordering::Relaxed);
    let scrolls = counters.scroll_steps.load(Ordering::Relaxed);
    let touch = counters.touch_taps.load(Ordering::Relaxed);
    let total = keys + clicks + scrolls + touch;

    let secs = duration.as_secs();
    let mins = secs / 60;
    let secs_remainder = secs % 60;

    let duration_str = if mins > 0 {
        format!("{}m {}s", mins, secs_remainder)
    } else {
        format!("{}s", secs)
    };

    let apm = if duration.as_secs_f64() > 0.0 {
        (total as f64 / duration.as_secs_f64()) * 60.0
    } else {
        0.0
    };

    eprintln!("\n=== Action Summary ===");
    eprintln!("Duration: {}", duration_str);
    eprintln!("Key presses: {}", keys);
    eprintln!("Button clicks: {}", clicks);
    eprintln!("Scroll steps: {} (tracked separately)", scrolls);
    eprintln!("Touch taps: {}", touch);
    eprintln!("Total actions: {} (keys + clicks)", total);
    eprintln!("Actions per minute: {:.1}", apm);
}

// Handler implementations

struct WlDisplayHandlerImpl {
    counters: Arc<ActionCounters>,
    pressed_keys: Arc<Mutex<HashSet<u32>>>,
    pressed_buttons: Arc<Mutex<HashSet<u32>>>,
    last_scroll_time: Arc<Mutex<Instant>>,
}

impl WlDisplayHandler for WlDisplayHandlerImpl {
    fn handle_get_registry(&mut self, slf: &Rc<WlDisplay>, registry: &Rc<WlRegistry>) {
        registry.set_handler(WlRegistryHandlerImpl {
            counters: self.counters.clone(),
            pressed_keys: self.pressed_keys.clone(),
            pressed_buttons: self.pressed_buttons.clone(),
            last_scroll_time: self.last_scroll_time.clone(),
        });
        slf.send_get_registry(registry);
    }
}

struct WlRegistryHandlerImpl {
    counters: Arc<ActionCounters>,
    pressed_keys: Arc<Mutex<HashSet<u32>>>,
    pressed_buttons: Arc<Mutex<HashSet<u32>>>,
    last_scroll_time: Arc<Mutex<Instant>>,
}

impl WlRegistryHandler for WlRegistryHandlerImpl {
    fn handle_global(
        &mut self,
        slf: &Rc<WlRegistry>,
        name: u32,
        interface: ObjectInterface,
        version: u32,
    ) {
        slf.send_global(name, interface, version);
    }

    fn handle_global_remove(&mut self, slf: &Rc<WlRegistry>, name: u32) {
        slf.send_global_remove(name);
    }

    fn handle_bind(&mut self, slf: &Rc<WlRegistry>, name: u32, object: Rc<dyn Object>) {
        // Set counting handlers for wl_seat
        if object.core().interface() == ObjectInterface::WlSeat
            && let Ok(seat) = (object.clone() as Rc<dyn Any>).downcast::<WlSeat>()
        {
            eprintln!("[DEBUG] Creating seat handler");
            seat.set_handler(CountingSeatHandler {
                counters: self.counters.clone(),
                pressed_keys: self.pressed_keys.clone(),
                pressed_buttons: self.pressed_buttons.clone(),
                last_scroll_time: self.last_scroll_time.clone(),
            });
        }
        slf.send_bind(name, object);
    }
}

struct CountingSeatHandler {
    counters: Arc<ActionCounters>,
    pressed_keys: Arc<Mutex<HashSet<u32>>>,
    pressed_buttons: Arc<Mutex<HashSet<u32>>>,
    last_scroll_time: Arc<Mutex<Instant>>,
}

impl WlSeatHandler for CountingSeatHandler {
    fn handle_get_pointer(&mut self, slf: &Rc<WlSeat>, id: &Rc<WlPointer>) {
        use std::sync::atomic::AtomicU64;
        static POINTER_COUNTER: AtomicU64 = AtomicU64::new(0);
        let ptr_id = POINTER_COUNTER.fetch_add(1, Ordering::Relaxed);
        eprintln!("[DEBUG] Creating pointer handler #{}", ptr_id);
        id.set_handler(CountingPointerHandler {
            counters: self.counters.clone(),
            pressed_buttons: self.pressed_buttons.clone(),
            last_scroll_time: self.last_scroll_time.clone(),
            handler_id: ptr_id,
        });
        slf.send_get_pointer(id);
    }

    fn handle_get_keyboard(&mut self, slf: &Rc<WlSeat>, id: &Rc<WlKeyboard>) {
        id.set_handler(CountingKeyboardHandler {
            counters: self.counters.clone(),
            pressed_keys: self.pressed_keys.clone(),
        });
        slf.send_get_keyboard(id);
    }

    fn handle_get_touch(&mut self, slf: &Rc<WlSeat>, id: &Rc<WlTouch>) {
        id.set_handler(CountingTouchHandler {
            counters: self.counters.clone(),
        });
        slf.send_get_touch(id);
    }
}

struct CountingKeyboardHandler {
    counters: Arc<ActionCounters>,
    pressed_keys: Arc<Mutex<HashSet<u32>>>,
}

impl WlKeyboardHandler for CountingKeyboardHandler {
    fn handle_key(
        &mut self,
        slf: &Rc<WlKeyboard>,
        serial: u32,
        time: u32,
        key: u32,
        state: WlKeyboardKeyState,
    ) {
        // Track key state to distinguish initial press from repeats
        // Use shared pressed_keys so multiple handlers don't double-count
        match state {
            WlKeyboardKeyState::PRESSED => {
                let mut pressed = self.pressed_keys.lock().unwrap();
                // Only count if this key wasn't already pressed (ignore repeats and duplicates)
                if pressed.insert(key) {
                    self.counters.key_presses.fetch_add(1, Ordering::Relaxed);
                }
            }
            WlKeyboardKeyState::RELEASED => {
                let mut pressed = self.pressed_keys.lock().unwrap();
                // Remove from pressed set when released
                pressed.remove(&key);
            }
            _ => {}
        }
        slf.send_key(serial, time, key, state);
    }
}

struct CountingPointerHandler {
    counters: Arc<ActionCounters>,
    pressed_buttons: Arc<Mutex<HashSet<u32>>>,
    last_scroll_time: Arc<Mutex<Instant>>,
    handler_id: u64,
}

impl WlPointerHandler for CountingPointerHandler {
    fn handle_button(
        &mut self,
        slf: &Rc<WlPointer>,
        serial: u32,
        time: u32,
        button: u32,
        state: WlPointerButtonState,
    ) {
        // Track button state to avoid double-counting with multiple handlers
        match state {
            WlPointerButtonState::PRESSED => {
                let mut pressed = self.pressed_buttons.lock().unwrap();
                // Only count if this button wasn't already pressed
                let was_new = pressed.insert(button);
                eprintln!(
                    "[DEBUG] Handler #{}: Button {} pressed, was_new={}, count now={}",
                    self.handler_id,
                    button,
                    was_new,
                    self.counters.button_clicks.load(Ordering::Relaxed)
                        + if was_new { 1 } else { 0 }
                );
                if was_new {
                    self.counters.button_clicks.fetch_add(1, Ordering::Relaxed);
                }
            }
            WlPointerButtonState::RELEASED => {
                let mut pressed = self.pressed_buttons.lock().unwrap();
                eprintln!(
                    "[DEBUG] Handler #{}: Button {} released",
                    self.handler_id, button
                );
                pressed.remove(&button);
            }
            _ => {}
        }
        slf.send_button(serial, time, button, state);
    }

    fn handle_axis(&mut self, slf: &Rc<WlPointer>, time: u32, axis: WlPointerAxis, value: Fixed) {
        // Basic axis events for scrolling - throttle to avoid counting every micro-event
        if matches!(
            axis,
            WlPointerAxis::VERTICAL_SCROLL | WlPointerAxis::HORIZONTAL_SCROLL
        ) {
            let mut last_time = self.last_scroll_time.lock().unwrap();
            let now = Instant::now();
            let elapsed = now.duration_since(*last_time);

            // Only count if at least 100ms has passed since last scroll (debounce)
            if elapsed >= Duration::from_millis(100) {
                eprintln!(
                    "[DEBUG] Handler #{}: Axis scroll event (value={}, counted)",
                    self.handler_id,
                    value.to_f64()
                );
                self.counters.scroll_steps.fetch_add(1, Ordering::Relaxed);
                *last_time = now;
            } else {
                eprintln!(
                    "[DEBUG] Handler #{}: Axis scroll event (value={}, ignored - too soon)",
                    self.handler_id,
                    value.to_f64()
                );
            }
        }
        slf.send_axis(time, axis, value);
    }

    fn handle_axis_discrete(&mut self, slf: &Rc<WlPointer>, axis: WlPointerAxis, discrete: i32) {
        // Discrete scroll - throttle to avoid double counting
        let mut last_time = self.last_scroll_time.lock().unwrap();
        let now = Instant::now();
        let elapsed = now.duration_since(*last_time);

        if elapsed >= Duration::from_millis(100) {
            eprintln!(
                "[DEBUG] Handler #{}: Discrete scroll (discrete={}, counted)",
                self.handler_id, discrete
            );
            self.counters.scroll_steps.fetch_add(1, Ordering::Relaxed);
            *last_time = now;
        } else {
            eprintln!(
                "[DEBUG] Handler #{}: Discrete scroll (discrete={}, ignored - too soon)",
                self.handler_id, discrete
            );
        }
        slf.send_axis_discrete(axis, discrete);
    }

    fn handle_axis_value120(&mut self, slf: &Rc<WlPointer>, axis: WlPointerAxis, value120: i32) {
        // High-resolution scroll - throttle to avoid double counting
        let mut last_time = self.last_scroll_time.lock().unwrap();
        let now = Instant::now();
        let elapsed = now.duration_since(*last_time);

        if elapsed >= Duration::from_millis(100) {
            eprintln!(
                "[DEBUG] Handler #{}: Value120 scroll (value120={}, counted)",
                self.handler_id, value120
            );
            self.counters.scroll_steps.fetch_add(1, Ordering::Relaxed);
            *last_time = now;
        } else {
            eprintln!(
                "[DEBUG] Handler #{}: Value120 scroll (value120={}, ignored - too soon)",
                self.handler_id, value120
            );
        }
        slf.send_axis_value120(axis, value120);
    }
}

struct CountingTouchHandler {
    counters: Arc<ActionCounters>,
}

impl WlTouchHandler for CountingTouchHandler {
    fn handle_down(
        &mut self,
        slf: &Rc<WlTouch>,
        serial: u32,
        time: u32,
        surface: &Rc<WlSurface>,
        id: i32,
        x: Fixed,
        y: Fixed,
    ) {
        // Count each touch down as an action
        self.counters.touch_taps.fetch_add(1, Ordering::Relaxed);
        slf.send_down(serial, time, surface, id, x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_counters_basic() {
        let counters = ActionCounters::new();

        // Test initial state
        assert_eq!(counters.key_presses.load(Ordering::Relaxed), 0);
        assert_eq!(counters.button_clicks.load(Ordering::Relaxed), 0);
        assert_eq!(counters.scroll_steps.load(Ordering::Relaxed), 0);
        assert_eq!(counters.touch_taps.load(Ordering::Relaxed), 0);
        assert_eq!(counters.total(), 0);

        // Test incrementing
        counters.key_presses.fetch_add(5, Ordering::Relaxed);
        counters.button_clicks.fetch_add(3, Ordering::Relaxed);
        assert_eq!(counters.key_presses.load(Ordering::Relaxed), 5);
        assert_eq!(counters.button_clicks.load(Ordering::Relaxed), 3);
        assert_eq!(counters.total(), 8);
    }

    #[test]
    fn test_shared_pressed_keys() {
        // Test that shared pressed_keys prevents double-counting
        let pressed_keys = Arc::new(Mutex::new(HashSet::new()));
        let counters = Arc::new(ActionCounters::new());

        // Simulate two handlers processing the same key press
        {
            let mut pressed = pressed_keys.lock().unwrap();
            if pressed.insert(42) {
                counters.key_presses.fetch_add(1, Ordering::Relaxed);
            }
        }

        {
            let mut pressed = pressed_keys.lock().unwrap();
            if pressed.insert(42) {
                counters.key_presses.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Should only count once
        assert_eq!(counters.key_presses.load(Ordering::Relaxed), 1);

        // Release the key
        {
            let mut pressed = pressed_keys.lock().unwrap();
            pressed.remove(&42);
        }

        // Press again should count
        {
            let mut pressed = pressed_keys.lock().unwrap();
            if pressed.insert(42) {
                counters.key_presses.fetch_add(1, Ordering::Relaxed);
            }
        }

        assert_eq!(counters.key_presses.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_shared_pressed_buttons() {
        // Test that shared pressed_buttons prevents double-counting clicks
        let pressed_buttons = Arc::new(Mutex::new(HashSet::new()));
        let counters = Arc::new(ActionCounters::new());

        // Simulate two handlers processing the same button press
        {
            let mut pressed = pressed_buttons.lock().unwrap();
            if pressed.insert(272) {
                // BTN_LEFT
                counters.button_clicks.fetch_add(1, Ordering::Relaxed);
            }
        }

        {
            let mut pressed = pressed_buttons.lock().unwrap();
            if pressed.insert(272) {
                counters.button_clicks.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Should only count once
        assert_eq!(counters.button_clicks.load(Ordering::Relaxed), 1);

        // Release the button
        {
            let mut pressed = pressed_buttons.lock().unwrap();
            pressed.remove(&272);
        }

        // Press again should count
        {
            let mut pressed = pressed_buttons.lock().unwrap();
            if pressed.insert(272) {
                counters.button_clicks.fetch_add(1, Ordering::Relaxed);
            }
        }

        assert_eq!(counters.button_clicks.load(Ordering::Relaxed), 2);
    }
}
