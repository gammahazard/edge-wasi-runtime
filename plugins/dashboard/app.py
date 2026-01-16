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
    
    the rust host calls render(temperature, humidity) when a browser
    requests the dashboard page. we return complete html.
    
    wit signature:
        render: func(temperature: f32, humidity: f32) -> string
    
    python signature:
        def render(self, temperature: float, humidity: float) -> str
    """
    
    def render(self, temperature: float, humidity: float) -> str:
        """
        render a complete html dashboard page with the given sensor readings.
        
        args:
            temperature: current temperature in celsius (f32 -> float)
            humidity: current relative humidity percentage (f32 -> float)
            
        returns:
            complete html document as a string
            (including <!doctype html>)
            
        called by:
            rust host's dashboard_handler in main.rs
            
        design notes:
            - we return complete html, not fragments (simpler for demo)
            - css is inline to avoid asset serving complexity
            - auto-refresh via meta tag for live updates
            - dark theme with modern styling for visual appeal
        """
        
        
        # determine status styling based on values
        # this shows python logic running in wasm
        temp_class = "reading temp"
        if temperature > 30.0:
            temp_class += " danger"
        elif temperature < 10.0:
            temp_class += " cold"
        
        humidity_class = "reading humidity"
        if humidity > 80.0:
            humidity_class += " danger"
        elif humidity < 20.0:
            humidity_class += " warning"
        
        # build the complete html page
        # using f-string for templating (simple and fast)
        # NOTE: CSS brackets {} must be doubled {{}} to escape specific styles
        html = f"""<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>wasi python dashboard</title>
    
    <!-- inline css - complete design system -->
    <style>
        /* ==== design tokens (CYBERPUNK THEME) ==== */
        :root {{
            --bg-primary: #050008;  /* Deep black/purple */
            --bg-card: #150020;     /* Rich purple */
            --bg-hover: #250035;
            --accent: #ff00ff;      /* NEON MAGENTA */
            --text-primary: #ffffff;
            --text-secondary: #d0a0ff;
            --success: #00ffff;     /* CYAN */
            --warning: #ffaa00;
            --danger: #ff0055;
            --cold: #66bbff;
            --border-subtle: #440055;
            --shadow: 0 4px 24px rgba(255,0,255,0.1);
            --shadow-hover: 0 0 32px rgba(255,0,255,0.3);
        }}
        
        /* ==== base reset ==== */
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        
        body {{
            font-family: 'courier new', monospace; /* HACKER FONT */
            background: var(--bg-primary);
            color: var(--text-primary);
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            padding: 2rem;
            line-height: 1.5;
            background-image: linear-gradient(0deg, transparent 24%, rgba(255, 0, 255, .03) 25%, rgba(255, 0, 255, .03) 26%, transparent 27%, transparent 74%, rgba(255, 0, 255, .03) 75%, rgba(255, 0, 255, .03) 76%, transparent 77%, transparent), linear-gradient(90deg, transparent 24%, rgba(255, 0, 255, .03) 25%, rgba(255, 0, 255, .03) 26%, transparent 27%, transparent 74%, rgba(255, 0, 255, .03) 75%, rgba(255, 0, 255, .03) 76%, transparent 77%, transparent);
            background-size: 50px 50px;
        }}
        
        /* ==== header ==== */
        .header {{
            text-align: center;
            margin-bottom: 2rem;
        }}
        
        .header h1 {{
            font-size: 2.5rem;
            font-weight: 700;
            color: var(--accent);
            text-shadow: 0 0 10px var(--accent); /* NEON GLOW */
            margin-bottom: 0.5rem;
            text-transform: uppercase;
            letter-spacing: 0.1em;
        }}
        
        .header p {{
            color: var(--text-secondary);
        }}
        
        /* ==== badges ==== */
        .badge {{
            display: inline-block;
            background: rgba(255, 0, 255, 0.1);
            padding: 4px 12px;
            font-size: 0.75rem;
            color: var(--success);
            border: 1px solid var(--success);
            margin: 0.25rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            box-shadow: 0 0 5px var(--success);
        }}
        
        /* ==== sensor cards with POLISH ==== */
        .grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 2rem;
            max-width: 900px;
            width: 100%;
        }}
        
        .card {{
            background: var(--bg-card);
            border-radius: 0; /* BOXIER Cyberpunk look */
            padding: 2rem;
            border: 2px solid var(--accent); /* NEON BORDER */
            box-shadow: var(--shadow);
            transition: all 0.2s steps(4); /* Glitchy transition */
            position: relative;
            overflow: hidden;
        }}
        
        /* Scanline effect */
        .card::before {{
            content: '';
            position: absolute;
            top: 0;
            left: 0;
            width: 100%;
            height: 100%;
            background: linear-gradient(transparent 50%, rgba(0,0,0,0.3) 50%);
            background-size: 100% 4px;
            pointer-events: none;
            opacity: 0.3;
        }}
        
        .card:hover {{
            transform: translate(-4px, -4px);
            box-shadow: 4px 4px 0 var(--success); /* Retro shadow */
            background: var(--bg-hover);
        }}
        
        .card-header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 1.5rem;
            color: var(--success);
            font-size: 1rem;
            text-transform: uppercase;
            letter-spacing: 0.15em;
            font-weight: 600;
            border-bottom: 1px dashed var(--success);
            padding-bottom: 0.5rem;
        }}
        
        .card-icon {{
            font-size: 1.5rem;
        }}
        
        /* ==== readings ==== */
        .reading {{
            font-size: 3.5rem;
            font-weight: 700;
            text-shadow: 0 0 15px currentColor; /* GLOWING NUMBERS */
        }}
        
        .unit {{
            font-size: 1.5rem;
            color: var(--text-secondary);
            font-weight: 400;
            text-shadow: none;
        }}
        
        .temp {{ color: var(--accent); }}
        .humidity {{ color: var(--success); }}
        .cold {{ color: var(--cold); }}
        .warning {{ color: var(--warning); }}
        .danger {{ color: var(--danger); }}
        
        /* ==== footer ==== */
        .footer {{
            margin-top: 3rem;
            text-align: center;
            color: var(--text-secondary);
            font-size: 0.8rem;
        }}
        
        /* ==== footer architecture box (UPDATED) ==== */
        .architecture {{
            margin: 2rem auto;
            max-width: 600px;
            padding: 1.5rem;
            background: rgba(0,0,0,0.5);
            font-family: 'consolas', 'monaco', monospace;
            font-size: 0.8rem;
            color: var(--success);
            border: 1px solid var(--success);
            box-shadow: 0 0 10px rgba(0,255,255,0.1);
            position: relative;
        }}
        
        .architecture code {{
            color: var(--accent);
            font-weight: bold;
        }}
        
        /* ==== responsive ==== */
        @media (max-width: 600px) {{
            body {{ padding: 1rem; }}
            .header h1 {{ font-size: 1.75rem; }}
            .reading {{ font-size: 2.5rem; }}
        }}
    </style>
    
    <!-- auto-refresh every 2 seconds to show live data -->
    <meta http-equiv="refresh" content="2">
</head>
<body>
    <header class="header">
        <h1>// SYSTEM_DASHBOARD</h1>
        <p>
            <span class="badge">host::rust</span>
            <span class="badge">guest::python</span>
            <span class="badge">wasi::v0.2</span>
        </p>
    </header>
    
    <main class="grid">
        <article class="card">
            <header class="card-header">
                <span>>> TEMPERATURE</span>
                <span class="card-icon">[T]</span>
            </header>
            <div class="{temp_class}">
                {temperature:.1f}<span class="unit">&deg;C</span>
            </div>
        </article>
        
        <article class="card">
            <header class="card-header">
                <span>>> HUMIDITY</span>
                <span class="card-icon">[H]</span>
            </header>
            <div class="{humidity_class}">
                {humidity:.1f}<span class="unit">%</span>
            </div>
        </article>
    </main>
    
    <footer class="footer">
        <p>STATUS: <strong>ONLINE</strong> | RENDERER: <strong>PYTHON_WASM</strong></p>
        <div class="architecture">
            flow: browser -> rust_host -> <code>render()</code> -> python_wasm -> html
        </div>
    </footer>
</body>
</html>"""
        
        return html


# ==============================================================================
# optional: local testing without wasm
# ==============================================================================
# uncomment to test the html output locally:
#
# if __name__ == "__main__":
#     dashboard = DashboardLogic()
#     html = dashboard.render(22.5, 45.0)
#     with open("test_output.html", "w") as f:
#         f.write(html)
#     print("wrote test_output.html")
