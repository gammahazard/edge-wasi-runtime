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
| **Plugin (Python)** | `host.read_bme680()` | `i2c.transfer(0x77, "D0", 1)` → `"61"` |
| **Interface (WIT)** | `read-bme680: func() -> reading` | `transfer: func(addr, hex-data, len) -> hex-string` |
| **Host (Rust)** | *Hardcoded BME680 logic* | *Blindly passes bytes to /dev/i2c-1* |

## 3. Implementation Status (Phase 3) ✅

### 3.1 WIT Interfaces
Added to `wit/plugin.wit`:

```wit
interface i2c {
    // Uses hex strings due to componentize-py marshalling limitations
    // Python: i2c.transfer(0x77, "D0", 1) -> "61"
    transfer: func(addr: u8, write-data: string, read-len: u32) -> result<string, string>;
}

interface spi {
    transfer: func(data: list<u8>) -> result<list<u8>, string>;
}

interface uart {
    read: func(max-len: u32) -> result<list<u8>, string>;
    write: func(data: list<u8>) -> result<u32, string>;
    set-baud: func(rate: u32) -> result<tuple<>, string>;
}
```

> **Note**: The `i2c` interface uses **hex-encoded strings** (e.g., `"D0"` instead of `[0xD0]`)
> due to a componentize-py marshalling issue with `list<u8>` return types.

### 3.2 Host Implementation
Added to `host/src/gpio.rs`:
- `i2c_transfer(addr, hex_data, len)` - Uses `rppal::i2c` + `hex` crate
- `spi_transfer(data)` - Uses `rppal::spi`
- `uart_read/write/set_baud()` - Uses `rppal::uart`
- **Docker Support**: `Dockerfile` and `docker-compose.yml` created (Phase 3.7).

Dependencies added:
- `rppal = "0.19"` (Raspberry Pi HAL)
- `hex = "0.4"` (hex encoding/decoding)

## 4. The "Hybrid" Compromise

### Critical Discovery
During implementation, we found that **some sensors have timing requirements** beyond what WASM can provide:

| Sensor | Issue | Solution |
|--------|-------|----------|
| **DHT22** | Microsecond bit-banging | Keep host driver (`read_dht22`) |
| **WS2812B** | 400ns pulse timing | Keep host driver (Python subprocess) |
| **BME680 Gas** | 100ms delay between trigger/read | Keep host driver (`read_bme680`) |
| **BME680 Temp/Humidity** | No timing issues | ✅ Works with Generic I2C |

### Key Insight
**WASM cannot call `time.sleep()`**. This means any sensor requiring:
- Microsecond timing (DHT22, WS2812B)
- Millisecond delays (BME680 gas heater warmup)

...must remain as host-side drivers.

### Generic-Friendly Sensors (Verified)
| Sensor | Protocol | Status |
|--------|----------|--------|
| **SSD1306 OLED** | I2C | ✅ Implemented (plugins/oled) |
| **AHT20** | I2C | ✅ Should work |
| **BMP280** | I2C | ✅ Should work (no gas) |
| **APA102 / DotStar** | SPI (has clock) | ✅ Should work |

## 5. Phase 4: Decoupled UI (JSON Broadcast) ✅

To enable "Drop & Run" for new sensors, the Dashboard and OLED plugins must not require WIT changes.

**Solution: The "Broadcast" Pattern**
1. **Host** collects readings -> Serializes to JSON (`serde_json`)
2. **Host** calls `render(json)` on Dashboard
3. **Host** calls `update(json)` on OLED
4. **Plugins** parse JSON and decide what to show

**Status**: Implemented.
- `render: func(sensor-data: string)`
- `update: func(sensor-data: string)`
- No Host/WIT recompilation needed for UI changes!

## 6. Phase 5: Security & Dynamic Loading ⏳

Giving generic "Raw I/O" (I2C/GPIO) access is powerful but adds risk. Also, we want to auto-load plugins.

**Planned Architecture:**

1.  **Permission System (`permission.toml`)**:
    -   Gatekeeper checking every `i2c.transfer()` call.
    -   Policy: Deny by default.
    ```toml
    [plugins.oled]
    allow_i2c = [0x3C] # ✅ Allowed
    allow_gpio = []    # ❌ Blocked
    ```

2.  **Dynamic Discovery**:
    -   Watch `plugins/` folder.
    -   Auto-load `.wasm` files.
    -   Apply permissions from toml immediately.

**Status**: Planned for next phase.

## 7. Lessons Learned

1. **componentize-py has marshalling issues** with `list<u8>` return types
   - Workaround: Use hex-encoded strings

2. **WASM cannot sleep()** - timing-critical operations must stay in host
   - This is a fundamental WASM limitation, not a bug

3. **Generic HAL still provides value** for:
   - Read-only sensors (temp, pressure, light)
   - Write-only devices (OLED displays, SPI LEDs)
   - Any device without strict timing requirements
