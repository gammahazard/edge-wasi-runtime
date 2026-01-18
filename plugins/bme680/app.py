"""
==============================================================================
bme680_plugin.py - BME680 environmental sensor logic
==============================================================================

purpose:
    polls the BME680 sensor via the host capability.
    manages LED 2 for air quality status.
    exports readings to the host.

relationships:
    - implements: ../../../wit/plugin.wit (bme680-logic interface)
    - imports: gpio-provider, led-controller
    - loaded by: host/src/runtime.rs
    
build command:
    componentize-py -d ../../wit -w bme680-plugin componentize app -o bme680.wasm
"""

# ==============================================================================
# wit-generated imports
# ==============================================================================
import wit_world
from wit_world.imports import gpio_provider, led_controller
from wit_world.exports import Bme680Logic
from wit_world.exports.bme680_logic import Bme680Reading

# ==============================================================================
# IAQ State (Persisted via Stateful Runtime)
# ==============================================================================
gas_baseline = 0.0      # Tracks cleanest air seen (in KOhms)
burn_in_count = 0       # Calibration counter (12 polls = 60 seconds)
gas_history = []        # Rolling history for smoothing

class Bme680Logic(Bme680Logic):
    def poll(self) -> list[Bme680Reading]:
        """
        Read BME680 sensor and calculate Indoor Air Quality (IAQ) score.
        
        IAQ Scale (Bosch BSEC-inspired):
          0-50   = Excellent (Green)
          51-100 = Good (Green)
          101-150 = Moderate (Yellow/Orange)  
          151-200 = Poor (Orange)
          201-300 = Bad (Red)
          301-500 = Hazardous (Red)
        
        Gas Resistance Meaning:
          HIGH (100+ KOhm) = Clean air
          LOW (1-50 KOhm) = Polluted air (VOCs present)
        """
        global gas_baseline, burn_in_count, gas_history
        readings = []
        
        try:
            result = gpio_provider.read_bme680(0x77)
            
            if isinstance(result, tuple):
                temp, humidity, pressure, gas = result
                timestamp = gpio_provider.get_timestamp_ms()
                
                burn_in_count += 1
                
                # Track gas history for smoothing (last 5 readings)
                gas_history.append(gas)
                if len(gas_history) > 5:
                    gas_history.pop(0)
                avg_gas = sum(gas_history) / len(gas_history)
                
                # --- IMPROVED IAQ ALGORITHM ---
                
                # CALIBRATION PHASE (first 60 seconds)
                if burn_in_count < 12:
                    # During warm-up, track the baseline but don't calculate IAQ
                    if gas > gas_baseline:
                        gas_baseline = gas
                    
                    current_iaq = 0
                    iaq_accuracy = 0
                    print(f"🟣 [CALIBRATING] Warming up... ({burn_in_count}/12) | Gas: {gas:.1f} KΩ | Baseline: {gas_baseline:.1f} KΩ")
                    led_controller.set_led(2, 255, 0, 255)  # Purple
                else:
                    # ACTIVE PHASE - Calculate IAQ
                    
                    # 1. Slowly drift baseline toward current reading (adaptive)
                    #    This handles environmental changes over time.
                    if gas > gas_baseline:
                        gas_baseline = gas  # New clean air peak
                    else:
                        # Decay baseline slowly (0.1% per reading) to adapt
                        gas_baseline = gas_baseline * 0.999 + gas * 0.001
                    
                    # 2. Gas Score (0-75 points)
                    #    Lower resistance = more pollution = higher score
                    if gas_baseline > 0:
                        # Ratio of current to baseline (1.0 = baseline, <1.0 = degraded)
                        gas_ratio = gas / gas_baseline
                        # Invert: 1.0 baseline = 0 score, 0.5 baseline = 37.5 score
                        gas_score = (1.0 - gas_ratio) * 75.0
                        gas_score = max(0, min(75, gas_score))  # Clamp to 0-75
                    else:
                        gas_score = 0
                    
                    # 3. Humidity Score (0-25 points)
                    #    Ideal humidity is 40%. Deviation adds to score.
                    hum_offset = abs(humidity - 40.0)
                    hum_score = (hum_offset / 40.0) * 25.0
                    hum_score = min(25, hum_score)  # Clamp to 0-25
                    
                    # 4. Final IAQ (0-500)
                    current_iaq = (gas_score + hum_score) * 5.0
                    current_iaq = min(500, max(0, current_iaq))
                    iaq_accuracy = 1
                    
                    # LED Color based on IAQ
                    if current_iaq <= 50:
                        led_controller.set_led(2, 0, 255, 0)  # Green
                        status = "Excellent"
                    elif current_iaq <= 100:
                        led_controller.set_led(2, 0, 200, 50)  # Green-ish
                        status = "Good"
                    elif current_iaq <= 150:
                        led_controller.set_led(2, 255, 150, 0)  # Yellow
                        status = "Moderate"
                    elif current_iaq <= 200:
                        led_controller.set_led(2, 255, 100, 0)  # Orange
                        status = "Poor"
                    else:
                        led_controller.set_led(2, 255, 0, 0)  # Red
                        status = "Bad"
                    
                    # Debug output
                    print(f"🔵 [BME680] {temp:.1f}°C | {humidity:.0f}% | Gas: {gas:.0f} KΩ (Base: {gas_baseline:.0f}) | IAQ: {int(current_iaq)} ({status})")
                
                reading = Bme680Reading(
                    sensor_id="bme680-i2c-0x77",
                    temperature=temp,
                    humidity=humidity,
                    pressure=pressure,
                    gas_resistance=gas,
                    iaq_score=int(current_iaq),
                    iaq_accuracy=iaq_accuracy,
                    timestamp_ms=timestamp
                )
                readings.append(reading)
                    
            else:
                led_controller.set_led(2, 255, 0, 255)
                print(f"⚠️ BME680 read error: {result}")
                
        except Exception as e:
            led_controller.set_led(2, 255, 0, 255)
            print(f"❌ BME680 Exception: {e}")
            
        return readings

