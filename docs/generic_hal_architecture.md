# Generic HAL Architecture Design
**"Compile Once, Run Anything"**

## 1. The Problem
Currently, the Rust Host acts as a "Specialist". It knows exactly how to talk to a DHT22, BME680, and WS2812B.
- **Pros**: Easy to write initially, timing-critical stuff handled in Rust.
- **Cons**: Adding a new sensor requires **recompiling the Host**. This violates the "Compile Once" vision.

## 2. The Solution: Generic HAL
We transform the Host into a "Generalist" (Operating System). It strictly provides access to hardware buses, and plugins act as the "Drivers".

### Architecture Shift
| Layer | Current (Specialist) | Future (Generic HAL) |
|-------|----------------------|----------------------|
| **Plugin (Python)** | `host.read_bme680()` | `i2c.write(0x77, [0xF4, 0x27])` |
| **Interface (WIT)** | `read-bme680: func() -> reading` | `i2c-write: func(addr: u8, bytes: list<u8>)` |
| **Host (Rust)** | *Hardcoded generic BME680 logic* | *Blindly passes bytes to /dev/i2c-1* |

## 3. Migration Plan

### Phase 1: The Generic Interfaces (WIT)
We define the "syscalls" for our hardware.

```wit
interface gpio {
    enum mode { input, output }
    set-mode: func(pin: u8, m: mode);
    write: func(pin: u8, value: bool);
    read: func(pin: u8) -> bool;
}

interface i2c {
    // transaction: write bytes, then read bytes (atomic)
    transfer: func(addr: u8, write: list<u8>, read-len: u32) -> list<u8>;
}

interface spi {
    transfer: func(data: list<u8>) -> list<u8>; // Full duplex
}
```

### Phase 2: Host Implementation (rppal / linux-embedded-hal)
The Host removes specific driver logic (python scripts) and uses `rppal` (Rust Raspberry Pi HAL) to talk properly to the kernel.

**New `host/Cargo.toml` dependencies:**
- `rppal = "0.14"` (GPIO, I2C, SPI, PWM)

**New `gpio.rs` logic:**
```rust
// No more "read_bme680". Just "i2c_transfer".
pub fn i2c_transfer(addr: u16, data: &[u8]) -> Vec<u8> {
    let mut i2c = I2c::new().unwrap();
    i2c.set_slave_address(addr).unwrap();
    i2c.write(data).unwrap();
    // ...
}
```

### Phase 3: The "Hybrid" Compromise
**Critical Insight**: Some sensors (DHT22, WS2812B) require microsecond-level timing that WASM (running in a runtime) cannot guarantee.
- **Strategy**: Keep specific drivers *only* for timing-sensitive hardware.
- **Result**:
    - **I2C/SPI Sensors**: Fully generic (Python drivers).
    - **Timing Critical**: Host extensions (Keep `dht22-provider` interface).

## 4. Security Model (permission.toml)
Giving raw I2C access is dangerous. We need a permission system.
*Current*: Implicit trust.
*Future*: Config-based capabilities.

```toml
[plugins.bme680]
allowed_i2c = [0x76, 0x77] # Can only talk to these addresses
allowed_gpio = []
```

## 5. Step-by-Step Execution
1.  **Define WIT**: Add `gpio`, `i2c` interfaces to `plugin.wit`.
2.  **Impl Host**: Add `rppal` to Host and implement I2C/GPIO traits.
3.  **Port BME680**: Rewrite `plugins/bme680/app.py` to use `i2c.write` instead of calling `bme680-logic`.
4.  **Verify**: Ensure BME680 still works.
5.  **Cleanup**: Remove `read-bme680` from Host.

## 6. Benefits for Portfolio
- Demonstrates **Systems Architecture** (Kernel vs User-space).
- Shows understanding of **Real-Time Constraints** (Hybrid approach).
- True "Platform Engineering" vs just script writing.
