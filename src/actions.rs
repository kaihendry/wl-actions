use {
    crate::ActionsError,
    std::{
        any::Any,
        process::{Command, exit},
        rc::Rc,
        sync::{
            Arc,
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
        self.key_presses.load(Ordering::Relaxed)
            + self.button_clicks.load(Ordering::Relaxed)
            + self.scroll_steps.load(Ordering::Relaxed)
            + self.touch_taps.load(Ordering::Relaxed)
    }
}

pub fn main(quiet: bool, program: Vec<String>) -> Result<(), ActionsError> {
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
                    "\rKeys: {} | Clicks: {} | Scrolls: {} | Touch: {} | Total: {}    ",
                    keys, clicks, scrolls, touch, total
                );
                thread::sleep(Duration::from_millis(100));
            }
        });
    }

    // Run the proxy - this will block until the child exits or server errors
    let counters_for_handler = counters.clone();
    let err = server.run(move || WlDisplayHandlerImpl {
        counters: counters_for_handler.clone(),
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
    eprintln!("Scroll steps: {}", scrolls);
    eprintln!("Touch taps: {}", touch);
    eprintln!("Total actions: {}", total);
    eprintln!("Actions per minute: {:.1}", apm);
}

// Handler implementations

struct WlDisplayHandlerImpl {
    counters: Arc<ActionCounters>,
}

impl WlDisplayHandler for WlDisplayHandlerImpl {
    fn handle_get_registry(&mut self, slf: &Rc<WlDisplay>, registry: &Rc<WlRegistry>) {
        registry.set_handler(WlRegistryHandlerImpl {
            counters: self.counters.clone(),
        });
        slf.send_get_registry(registry);
    }
}

struct WlRegistryHandlerImpl {
    counters: Arc<ActionCounters>,
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
        if object.core().interface() == ObjectInterface::WlSeat {
            if let Ok(seat) = (object.clone() as Rc<dyn Any>).downcast::<WlSeat>() {
                seat.set_handler(CountingSeatHandler {
                    counters: self.counters.clone(),
                });
            }
        }
        slf.send_bind(name, object);
    }
}

struct CountingSeatHandler {
    counters: Arc<ActionCounters>,
}

impl WlSeatHandler for CountingSeatHandler {
    fn handle_get_pointer(&mut self, slf: &Rc<WlSeat>, id: &Rc<WlPointer>) {
        id.set_handler(CountingPointerHandler {
            counters: self.counters.clone(),
            // For high-res scrolling accumulation
            scroll_accumulator_v: 0,
            scroll_accumulator_h: 0,
        });
        slf.send_get_pointer(id);
    }

    fn handle_get_keyboard(&mut self, slf: &Rc<WlSeat>, id: &Rc<WlKeyboard>) {
        id.set_handler(CountingKeyboardHandler {
            counters: self.counters.clone(),
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
        // Only count key presses, not releases or repeats
        if state == WlKeyboardKeyState::PRESSED {
            self.counters.key_presses.fetch_add(1, Ordering::Relaxed);
        }
        slf.send_key(serial, time, key, state);
    }
}

struct CountingPointerHandler {
    counters: Arc<ActionCounters>,
    scroll_accumulator_v: i32,
    scroll_accumulator_h: i32,
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
        // Only count button presses, not releases
        if state == WlPointerButtonState::PRESSED {
            self.counters.button_clicks.fetch_add(1, Ordering::Relaxed);
        }
        slf.send_button(serial, time, button, state);
    }

    fn handle_axis_discrete(&mut self, slf: &Rc<WlPointer>, axis: WlPointerAxis, discrete: i32) {
        // Each discrete step counts as a scroll action
        self.counters
            .scroll_steps
            .fetch_add(discrete.unsigned_abs() as u64, Ordering::Relaxed);
        slf.send_axis_discrete(axis, discrete);
    }

    fn handle_axis_value120(&mut self, slf: &Rc<WlPointer>, axis: WlPointerAxis, value120: i32) {
        // Accumulate high-resolution scroll values
        // Each 120 units = 1 logical scroll step
        let accumulator = match axis {
            WlPointerAxis::VERTICAL_SCROLL => &mut self.scroll_accumulator_v,
            WlPointerAxis::HORIZONTAL_SCROLL => &mut self.scroll_accumulator_h,
            _ => {
                slf.send_axis_value120(axis, value120);
                return;
            }
        };

        *accumulator += value120;
        let steps = *accumulator / 120;
        if steps != 0 {
            self.counters
                .scroll_steps
                .fetch_add(steps.unsigned_abs() as u64, Ordering::Relaxed);
            *accumulator %= 120;
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
