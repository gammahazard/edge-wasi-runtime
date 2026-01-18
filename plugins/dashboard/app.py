"""
==============================================================================
dashboard_plugin.py - python dashboard plugin for wasi host
==============================================================================

purpose:
    this module implements the dashboard-logic interface defined in
    ../../../wit/plugin.wit. when compiled to wasm, the rust host calls
    its render() function to generate html for the web dashboard.

relationships:
    - implements: ../../../wit/plugin.wit (dashboard-logic interface)
    - loaded by: ../../../host/src/runtime.rs (WasmRuntime::render_dashboard)
    - called by: ../../../host/src/main.rs (http dashboard handler)

design rationale:
    why render html in python wasm instead of rust?
    
    1. separation of concerns
       - rust host: http, tls, routing (performance-critical)
       - python guest: templating, styling (flexibility-critical)
    
    2. hot reload workflow
       - edit this file
       - rebuild: componentize-py -d ../../wit -w dashboard-plugin componentize dashboard_plugin -o dashboard.wasm
       - refresh browser (host auto-reloads wasm files!)
       - no rust recompilation needed
    
    3. polyglot flexibility
       - could rewrite in rust later for smaller binary size (~1kb vs ~38mb)
       - interface stays the same (wit contract)
       - host code doesn't change at all

industry parallels:
    - shopify: liquid templates run sandboxed for merchant customization
    - cloudflare workers: edge rendering with wasm isolation
    - fermyon spin: http handlers in python wasm

build command:
    componentize-py -d ../../wit -w dashboard-plugin componentize dashboard_plugin -o dashboard.wasm

==============================================================================
"""

# ==============================================================================
# wit-generated imports
# ==============================================================================
# componentize-py generates wit_world from plugin.wit
# we inherit from DashboardLogic and implement render()

import wit_world
from wit_world.exports import DashboardLogic


class DashboardLogic(DashboardLogic):
    """
    implementation of the dashboard-logic interface from plugin.wit.
    
    the rust host calls render(temperature, humidity, cpu_temp) when a browser
    requests the dashboard page. we return complete html.
    
    wit signature:
        render: func(temperature: f32, humidity: f32, cpu-temp: f32) -> string
    
    python signature:
        def render(self, temperature: float, humidity: float, cpu_temp: float, pressure: float, gas: float) -> str
    """
    
    def render(self, dht_temp: float, dht_hum: float, bme_temp: float, bme_hum: float, cpu_temp: float, pressure: float, gas: float, iaq: int) -> str:
        """
        render a complete html dashboard page with comparison layout.
        """
        
        # dht colors
        dht_temp_class = "reading temp"
        if dht_temp > 30.0: dht_temp_class += " danger"
        elif dht_temp < 10.0: dht_temp_class += " cold"
        
        dht_hum_class = "reading humidity"
        if dht_hum > 80.0: dht_hum_class += " danger"
        elif dht_hum < 20.0: dht_hum_class += " warning"

        # bme colors
        bme_temp_class = "reading temp"
        if bme_temp > 30.0: bme_temp_class += " danger"
        elif bme_temp < 10.0: bme_temp_class += " cold"
        
        bme_hum_class = "reading humidity"
        if bme_hum > 80.0: bme_hum_class += " danger"
        elif bme_hum < 20.0: bme_hum_class += " warning"
        
        # cpu temp status
        cpu_class = "reading cpu"
        if cpu_temp > 70.0: cpu_class += " danger"
        elif cpu_temp > 50.0: cpu_class += " warning"

        # status checks for new metrics
        pres_display = f"{pressure:.1f} hPa" if pressure > 0 else "N/A"
        gas_display = f"{gas:.1f} KΩ" if gas > 0 else "N/A"
        
        # IAQ (Indoor Air Quality) Score Logic
        aq_status = "STABILIZING..."
        aq_class = "reading"
        
        if iaq > 0:
            if iaq <= 50:
                aq_status = "EXCELLENT"
                aq_class += " safe"
            elif iaq <= 100:
                aq_status = "GOOD"
                aq_class += " safe"
            elif iaq <= 150:
                aq_status = "MODERATE"
                aq_class += " warning"
            elif iaq <= 200:
                aq_status = "POOR"
                aq_class += " danger"
            else:
                aq_status = "HEAVILY POLLUTED"
                aq_class += " danger"
        elif gas > 0:
            aq_status = "CALIBRATING..."
            aq_class += " warning"
        
        html = f"""<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>wasi python dashboard</title>
    <style>
        /* ==== design tokens (TERMINAL/CRT THEME) ==== */
        :root {{
            --bg-primary: #0a0a0a;
            --bg-card: #111111;
            --bg-hover: #1a1a1a;
            --accent: #33ff33;
            --text-primary: #33ff33;
            --text-secondary: #22cc22;
            --success: #33ff33;
            --warning: #ffcc00;
            --danger: #ff3333;
            --cold: #66ccff;
            --border-subtle: #1a3a1a;
            --shadow: 0 0 10px rgba(51,255,51,0.1);
            --shadow-hover: 0 0 20px rgba(51,255,51,0.3);
        }}
        
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        
        body {{
            font-family: 'VT323', 'Courier New', monospace;
            background: var(--bg-primary);
            color: var(--text-primary);
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            padding: 2rem;
            line-height: 1.5;
            background-image: repeating-linear-gradient(0deg, rgba(0,0,0,0.15), rgba(0,0,0,0.15) 1px, transparent 1px, transparent 2px);
        }}
        
        .header {{
            text-align: center;
            margin-bottom: 2rem;
            border: 2px solid var(--accent);
            padding: 1rem;
            background: rgba(0,0,0,0.5);
            width: 100%; max-width: 900px;
        }}
        .header h1 {{ font-size: 2rem; margin-bottom: 0.5rem; text-shadow: 0 0 10px var(--accent); }}
        
        .grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(350px, 1fr));
            gap: 2rem;
            max-width: 900px;
            width: 100%;
            margin-bottom: 2rem;
        }}
        
        .card {{
            background: var(--bg-card);
            padding: 2rem;
            border: 2px solid var(--accent);
            box-shadow: var(--shadow);
            position: relative;
        }}
        .card:hover {{ box-shadow: var(--shadow-hover); background: var(--bg-hover); transform: translate(-2px, -2px); }}
        
        .card-header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 1rem;
            border-bottom: 1px solid var(--success);
            padding-bottom: 0.5rem;
            font-size: 1.2rem;
            font-weight: bold;
        }}
        
        .reading-row {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 0.5rem;
        }}
        .reading-label {{ font-size: 1rem; color: var(--text-secondary); }}
        .reading-val {{ font-size: 2.5rem; font-weight: bold; text-shadow: 0 0 5px currentColor; }}
        
        .divider {{
            width: 100%; max-width: 900px;
            border: 0; border-top: 1px dashed var(--accent);
            margin: 2rem 0;
            opacity: 0.5;
        }}
        
        .controls {{
            width: 100%; max-width: 900px;
            border: 1px solid var(--accent);
            padding: 1.5rem;
            text-align: center;
        }}
        
        .btn {{
            background: transparent; color: var(--accent); border: 2px solid var(--accent);
            padding: 0.8rem 1.5rem; margin: 0.5rem; cursor: pointer; font-family: inherit;
            text-transform: uppercase; font-weight: bold;
        }}
        .btn:hover {{ background: var(--accent); color: var(--bg-primary); }}

        .temp {{ color: var(--accent); }}
        .humidity {{ color: var(--success); }}
        .cpu {{ color: #ff9900; }}
        .danger {{ color: var(--danger); }}
        .warning {{ color: var(--warning); }}

        .badge {{ display: inline-block; padding: 2px 8px; border: 1px solid var(--accent); font-size: 0.8rem; margin-right: 5px; }}
    </style>
</head>
<body>
    <script>
        async function updateReadings() {{
            try {{
                const response = await fetch('/api');
                const data = await response.json();
                
                // Find sensors
                const dht = data.readings.find(r => !r.pressure);
                const bme = data.readings.find(r => r.pressure);
                
                if (dht) {{
                    document.getElementById('dht-temp').textContent = dht.temperature.toFixed(1);
                    document.getElementById('dht-hum').textContent = dht.humidity.toFixed(1);
                }}
                if (bme) {{
                    document.getElementById('bme-temp').textContent = bme.temperature.toFixed(1);
                    document.getElementById('bme-hum').textContent = bme.humidity.toFixed(1);
                    document.getElementById('pres').textContent = bme.pressure.toFixed(1) + " hPa";
                    document.getElementById('gas').textContent = bme.gas_resistance.toFixed(1) + " KΩ";
                    if (bme.iaq_score) {{
                        document.getElementById('iaq-val').textContent = bme.iaq_score;
                        // Basic status update logic mirrored in JS for 0-refresh feel
                        let status = "STABILIZING...";
                        if (bme.iaq_score <= 50) status = "EXCELLENT";
                        else if (bme.iaq_score <= 100) status = "GOOD";
                        else if (bme.iaq_score <= 150) status = "MODERATE";
                        else if (bme.iaq_score <= 200) status = "POOR";
                        else status = "HEAVILY POLLUTED";
                        document.getElementById('aq-status').textContent = "STATUS: " + status;
                    }}
                }}
            }} catch (e) {{ console.error(e); }}
        }}
        setInterval(updateReadings, 2000);
        
        async function buzzerAction(a) {{ fetch('/api/buzzer?action='+a, {{method:'POST'}}); }}
    </script>
    
    <header class="header">
        <h1>// SENSOR DASHBOARD</h1>
        <div><span class="badge">HOST: RUST</span> <span class="badge">LOGIC: PYTHON</span> <span class="badge">WASM</span></div>
    </header>

    <!-- COMPARISON ROW -->
    <div class="grid">
        <article class="card">
            <header class="card-header">
                <span>DHT22 SENSOR</span>
                <span>[A]</span>
            </header>
            <div class="reading-row">
                <span class="reading-label">TEMP</span>
                <span id="dht-temp" class="{dht_temp_class} reading-val">{dht_temp:.1f}</span><span class="unit">&deg;C</span>
            </div>
            <div class="reading-row">
                <span class="reading-label">HUMIDITY</span>
                <span id="dht-hum" class="{dht_hum_class} reading-val">{dht_hum:.1f}</span><span class="unit">%</span>
            </div>
        </article>

        <article class="card">
            <header class="card-header">
                <span>BME680 SENSOR</span>
                <span>[B]</span>
            </header>
            <div class="reading-row">
                <span class="reading-label">TEMP</span>
                <span id="bme-temp" class="{bme_temp_class} reading-val">{bme_temp:.1f}</span><span class="unit">&deg;C</span>
            </div>
            <div class="reading-row">
                <span class="reading-label">HUMIDITY</span>
                <span id="bme-hum" class="{bme_hum_class} reading-val">{bme_hum:.1f}</span><span class="unit">%</span>
            </div>
        </article>
    </div>

    <!-- CPU TEMP (CENTERED) -->
    <div style="width:100%; max-width:900px; margin-bottom:2rem;">
        <article class="card" style="text-align: center;">
            <header class="card-header" style="justify-content: center;">>> CPU TEMPERATURE <<</header>
            <div class="{cpu_class} reading-val">{cpu_temp:.1f}<span class="unit">&deg;C</span></div>
        </article>
    </div>

    <hr class="divider">

    <!-- ENV DATA -->
    <div class="grid">
        <article class="card">
            <header class="card-header">PRESSURE</header>
            <div id="pres" class="reading-val" style="font-size: 2rem;">{pres_display}</div>
        </article>
        
    <article class="card">
            <header class="card-header">AIR QUALITY</header>
            <div id="gas" class="reading-val" style="font-size: 1.5rem; color: var(--text-secondary);">{gas_display} <span style="font-size: 0.8rem; vertical-align: middle;">(RAW)</span></div>
            <div id="iaq-val" class="{aq_class} reading-val" style="font-size: 3rem;">{iaq}</div>
            <div id="aq-status" style="margin-top:10px;">STATUS: {aq_status}</div>
        </article>
    </div>

    <hr class="divider">

    <section class="controls">
        <h2>>> BUZZER CONTROL <<</h2>
        <button class="btn" onclick="buzzerAction('beep')">BEEP</button>
        <button class="btn" onclick="buzzerAction('beep3')">3 BEEPS</button>
        <button class="btn" onclick="buzzerAction('long')">LONG (5s)</button>
    </section>
    
</body>
</html>"""
        return html


# ==============================================================================
# optional: local testing without wasm
# uncomment to test the html output locally:
#
# if __name__ == "__main__":
#     dashboard = DashboardLogic()
#     html = dashboard.render(22.5, 45.0)
#     with open("test_output.html", "w") as f:
#         f.write(html)
#     print("wrote test_output.html")
