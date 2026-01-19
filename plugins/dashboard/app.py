"""
==============================================================================
dashboard_plugin.py - python dashboard plugin for wasi host
==============================================================================

purpose:
    this module implements the dashboard-logic interface defined in
    ../../../wit/plugin.wit. when compiled to wasm, the rust host calls
    its render() function to generate html for the web dashboard.

design:
    renders a responsive HTML dashboard with 3 cards:
    1. DHT22
    2. BME680
    3. System Health (Pi Stats)
    
    theme: cyberpunk/terminal (vt323 font, scanlines, high contrast)
"""

import wit_world
from wit_world.exports import DashboardLogic


class DashboardLogic(DashboardLogic):
    """
    implementation of the dashboard-logic interface from plugin.wit.
    """
    
    def render(self, dht_temp: float, dht_hum: float, bme_temp: float, bme_hum: float, cpu_temp: float, memory_used_mb: int, memory_total_mb: int, uptime_seconds: int, pressure: float, gas: float, iaq: int) -> str:
        """
        Render the HTML dashboard with sensor data.
        """
        # Determine Status Colors
        dht_status = "ok" if dht_temp < 28.0 else "danger"
        bme_status = "ok" if iaq < 100 else "warning" if iaq < 200 else "danger"
        cpu_status = "ok" if cpu_temp < 60.0 else "warning" if cpu_temp < 75.0 else "danger"
        
        # Calculate uptime
        up_h = uptime_seconds // 3600
        up_m = (uptime_seconds % 3600) // 60
        uptime_str = f"{up_h}h {up_m}m"

        # IAQ Text Status
        aq_text = "CALIBRATING"
        if iaq > 0:
            if iaq <= 50: aq_text = "EXCELLENT"
            elif iaq <= 100: aq_text = "GOOD"
            elif iaq <= 150: aq_text = "MODERATE"
            elif iaq <= 200: aq_text = "POOR"
            else: aq_text = "BAD"
        elif gas > 0 and iaq == 0:
            aq_text = "CALIB..."

        return f"""
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>WASI SENSOR ARRAY</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=VT323&display=swap" rel="stylesheet">
    <style>
        :root {{
            --bg-color: #050505;
            --card-bg: #0a0a0a;
            --term-green: #33ff33;
            --term-dim: #1a801a;
            --term-alert: #ff3333;
            --term-warn: #ffcc00;
            --grid-line: #111;
        }}
        
        * {{ box-sizing: border-box; }}
        
        body {{
            background-color: var(--bg-color);
            background-image: 
                linear-gradient(rgba(18, 16, 16, 0) 50%, rgba(0, 0, 0, 0.25) 50%),
                linear-gradient(90deg, rgba(255, 0, 0, 0.06), rgba(0, 255, 0, 0.02), rgba(0, 0, 255, 0.06));
            background-size: 100% 2px, 3px 100%;
            color: var(--term-green);
            font-family: 'VT323', monospace;
            margin: 0;
            padding: 20px;
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            text-shadow: 0 0 2px var(--term-dim);
        }}

        .container {{
            width: 100%;
            max-width: 900px;
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
            gap: 20px;
            margin-top: 20px;
        }}

        .header {{
            text-align: center;
            margin-bottom: 30px;
            border-bottom: 1px solid var(--term-dim);
            width: 100%;
            max-width: 900px;
            padding-bottom: 20px;
        }}
        
        h1 {{
            margin: 0;
            font-size: 2.5rem;
            letter-spacing: 2px;
        }}
        
        .subtitle {{
            color: var(--term-dim);
            font-size: 1.2rem;
            text-transform: uppercase;
        }}

        .card {{
            background-color: var(--card-bg);
            border: 1px solid var(--term-green);
            padding: 20px;
            position: relative;
            box-shadow: 0 0 10px rgba(51, 255, 51, 0.1);
        }}
        
        /* Status Variants */
        .card.danger {{
            border-color: var(--term-alert);
            color: var(--term-alert);
            box-shadow: 0 0 10px rgba(255, 51, 51, 0.2);
        }}
        .card.danger .label {{ color: var(--term-alert); opacity: 0.7; }}
        .card.danger .value {{ text-shadow: 0 0 5px var(--term-alert); }}
        
        .card.warning {{
            border-color: var(--term-warn);
            color: var(--term-warn);
        }}
        .card.warning .value {{ text-shadow: 0 0 5px var(--term-warn); }}

        .label {{
            font-size: 1.2rem;
            margin-bottom: 10px;
            color: var(--term-green);
            opacity: 0.8;
            border-bottom: 1px dashed;
            padding-bottom: 5px;
            display: inline-block;
            width: 100%;
        }}

        .value {{
            font-size: 4rem;
            line-height: 1;
            margin: 15px 0;
            font-weight: normal;
        }}
        
        .unit {{ font-size: 2rem; vertical-align: super; opacity: 0.7; }}

        .metric-grid {{
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 10px;
            margin-top: 15px;
            border-top: 1px solid #222;
            padding-top: 15px;
        }}
        
        .metric {{ font-size: 1.2rem; }}
        .metric span {{ opacity: 0.6; display: block; font-size: 0.9rem; }}

        /* Scanline Animation */
        @keyframes scan {{
            0% {{ background-position: 0 0; }}
            100% {{ background-position: 0 100%; }}
        }}
        
        /* Controls Section */
        .controls {{
            width: 100%; 
            max-width: 900px;
            margin-top: 30px;
            border: 1px solid var(--term-dim);
            padding: 20px;
            display: flex;
            justify-content: center;
            gap: 15px;
            flex-wrap: wrap;
        }}
        
        .btn {{
            background: transparent;
            color: var(--term-green);
            border: 1px solid var(--term-green);
            font-family: 'VT323', monospace;
            font-size: 1.2rem;
            padding: 10px 20px;
            cursor: pointer;
            text-transform: uppercase;
            transition: all 0.1s;
        }}
        
        .btn:hover {{
            background: var(--term-green);
            color: #000;
            box-shadow: 0 0 15px var(--term-green);
        }}
        
        .btn:active {{ transform: translateY(2px); }}

    </style>
</head>
<body>
    <script>
        async function buzzerAction(a) {{ fetch('/api/buzzer?action='+a, {{method:'POST'}}); }}
        // Auto-refresh logic could go here, but meta-refresh or simple interval works
        setInterval(() => {{ window.location.reload(); }}, 5000); 
    </script>
    
    <div class="header">
        <h1>// SENSOR_ARRAY_V1</h1>
        <div class="subtitle">STATUS: ONLINE | UPTIME: {uptime_str}</div>
    </div>

    <div class="container">
        <!-- DHT22 -->
        <div class="card {dht_status}">
            <div class="label">>> DHT_22</div>
            <div class="value">{dht_temp:.1f}<span class="unit">°C</span></div>
            <div class="metric">HUMIDITY: {dht_hum:.1f}%</div>
        </div>

        <!-- BME680 -->
        <div class="card {bme_status}">
            <div class="label">>> BME_680</div>
            <div class="value">{bme_temp:.1f}<span class="unit">°C</span></div>
            <div class="metric-grid">
                <div class="metric"><span>HUM</span>{bme_hum:.0f}%</div>
                <div class="metric"><span>PRES</span>{pressure:.0f}hPa</div>
                <div class="metric"><span>GAS</span>{gas/1000:.1f}kΩ</div>
                <div class="metric"><span>IAQ</span>{iaq} ({aq_text})</div>
            </div>
        </div>

        <!-- SYSTEM -->
        <div class="card {cpu_status}">
            <div class="label">>> SYSTEM_CORE</div>
            <div class="value">{cpu_temp:.1f}<span class="unit">°C</span></div>
            <div class="metric-grid">
                <div class="metric"><span>RAM_USED</span>{memory_used_mb} MB</div>
                <div class="metric"><span>RAM_MAX</span>{memory_total_mb} MB</div>
            </div>
        </div>
    </div>

    <section class="controls">
        <button class="btn" onclick="buzzerAction('beep')">[ BEEP ONCE ]</button>
        <button class="btn" onclick="buzzerAction('beep3')">[ BEEP 3 TIMES ]</button>
        <button class="btn" onclick="buzzerAction('long')">[ LONG BEEP (5s) ]</button>
    </section>
</body>
</html>
"""
