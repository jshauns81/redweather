#!/usr/bin/env python3
import json
import os
import subprocess
import sys
import urllib.parse
import urllib.request
import tkinter as tk
from tkinter import ttk

API_KEY = os.environ.get("OWM_API_KEY")
CACHE_DIR = os.path.expanduser("~/.cache/redweather")
OVERRIDE_FILE = os.path.join(CACHE_DIR, "zip_override")


def geocode(query: str):
    if not API_KEY:
        return None, "Missing OWM_API_KEY"
    q = query.strip()
    if not q:
        return None, "Enter a ZIP or city"

    # Decide endpoint
    is_zip = q.replace(",", "").replace(" ", "").isdigit()
    if is_zip:
        zip_param = q if "," in q else f"{q},US"
        url = (
            "https://api.openweathermap.org/geo/1.0/zip?"
            + urllib.parse.urlencode({"zip": zip_param, "appid": API_KEY})
        )
    else:
        url = (
            "https://api.openweathermap.org/geo/1.0/direct?"
            + urllib.parse.urlencode({"q": q, "limit": 1, "appid": API_KEY})
        )

    try:
        with urllib.request.urlopen(url, timeout=8) as resp:
            body = resp.read()
            data = json.loads(body)
    except Exception as e:
        return None, f"Request error: {e}"

    if is_zip:
        if not isinstance(data, dict) or "lat" not in data or "lon" not in data:
            return None, "No result"
        name = data.get("name") or f"ZIP {q}"
        country = data.get("country")
        label = f"{name}, {country}" if country else name
        return {"label": label, "lat": data["lat"], "lon": data["lon"], "raw_query": q}, None
    else:
        if not isinstance(data, list) or not data:
            return None, "No result"
        first = data[0]
        name = first.get("name") or q
        country = first.get("country")
        state = first.get("state")
        label = name
        if country:
            label = f"{label}, {country}"
        if state:
            label = f"{label} ({state})"
        return {
            "label": label,
            "lat": first.get("lat"),
            "lon": first.get("lon"),
            "raw_query": q,
        }, None


def save_override(raw_query: str):
    os.makedirs(CACHE_DIR, exist_ok=True)
    with open(OVERRIDE_FILE, "w", encoding="utf-8") as f:
        f.write(raw_query.strip())


def reload_waybar():
    try:
        subprocess.run(["pkill", "-SIGUSR2", "waybar"], check=False)
    except Exception:
        pass


def main():
    root = tk.Tk()
    root.title("Set Weather Location")
    root.geometry("360x160")
    root.resizable(False, False)

    content = ttk.Frame(root, padding=10)
    content.pack(fill="both", expand=True)

    ttk.Label(content, text="ZIP or city,country:").grid(row=0, column=0, sticky="w")
    entry = ttk.Entry(content, width=30)
    entry.grid(row=0, column=1, sticky="we", padx=(6, 0))
    entry.focus()

    status_var = tk.StringVar(value="Enter location and press Check")
    status_label = ttk.Label(content, textvariable=status_var)
    status_label.grid(row=1, column=0, columnspan=2, sticky="w", pady=(6, 0))

    result_var = tk.StringVar(value="")
    result_label = ttk.Label(content, textvariable=result_var, font=("TkDefaultFont", 10, "bold"))
    result_label.grid(row=2, column=0, columnspan=2, sticky="w", pady=(6, 0))

    buttons = ttk.Frame(content)
    buttons.grid(row=3, column=0, columnspan=2, sticky="e", pady=(10, 0))
    save_btn = ttk.Button(buttons, text="Save", state="disabled")
    check_btn = ttk.Button(buttons, text="Check")
    cancel_btn = ttk.Button(buttons, text="Cancel", command=root.destroy)
    check_btn.grid(row=0, column=0, padx=4)
    save_btn.grid(row=0, column=1, padx=4)
    cancel_btn.grid(row=0, column=2, padx=4)

    current_raw = {"value": None}

    def do_check(*_):
        q = entry.get().strip()
        result_var.set("")
        save_btn.config(state="disabled")
        info, err = geocode(q)
        if err:
            status_var.set(err)
            return
        if not info:
            status_var.set("No result")
            return
        result_var.set(f"→ {info['label']}")
        status_var.set("OK")
        current_raw["value"] = info["raw_query"]
        save_btn.config(state="normal")

    def do_save():
        raw = current_raw.get("value")
        if not raw:
            status_var.set("Nothing to save")
            return
        save_override(raw)
        reload_waybar()
        root.destroy()

    check_btn.config(command=do_check)
    save_btn.config(command=do_save)
    entry.bind("<Return>", do_check)

    root.mainloop()


if __name__ == "__main__":
    main()
