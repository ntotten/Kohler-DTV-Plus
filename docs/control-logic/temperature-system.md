# Temperature System

How the DTV+ system represents, converts, and controls water and steam temperatures.

---

## Q-Format: Celsius x 2 (Cx2)

All water temperatures in the DTV+ system are stored as **Celsius multiplied by 2**. This is a fixed-point encoding that provides 0.5 degree C resolution using only integer arithmetic.

### Why Cx2?

The controller runs on a resource-constrained microcontroller with no hardware floating-point unit. Floating-point emulation is slow and increases code size. By doubling the Celsius value, every half-degree maps to an integer:

| Actual Temp | Cx2 Value | Notes                 |
| ----------- | --------- | --------------------- |
| 30.0 C      | 60        | Minimum system temp   |
| 35.0 C      | 70        |                       |
| 37.5 C      | 75        | Half-degree precision |
| 40.0 C      | 80        |                       |
| 43.0 C      | 86        |                       |
| 49.0 C      | 98        | Maximum water temp    |

This gives 0.5 degree C resolution with pure integer math -- no floating point needed.

---

## Temperature Limits

| Constant             | Cx2 Value | Actual Temp | Description                                 |
| -------------------- | --------- | ----------- | ------------------------------------------- |
| `MIN_SYS_VALVE_TEMP` | 60        | 30.0 C      | Below this, the Full Cold flag is set       |
| `MAX_WATER_TEMP`     | 98        | 49.0 C      | Absolute maximum water temperature setpoint |

When the actual temperature drops below `MIN_SYS_VALVE_TEMP` (Cx2 = 60), the system sets a "Full Cold" flag indicating the valve cannot achieve the requested temperature -- typically because the hot supply is unavailable or insufficient.

---

## This Is NOT PID Control

The DTV+ controller does **not** perform PID (proportional-integral-derivative) temperature regulation itself. The mixing valve contains its own internal control loop that blends hot and cold water to achieve a target temperature.

The DTV+ controller's role is limited to:

1. Sending a **temperature setpoint** (as a Cx2 value) to the valve.
2. Reading the **actual temperature** back from the valve's thermistor.
3. Reporting errors if the valve cannot reach the setpoint.

All proportional mixing logic runs inside the valve's own firmware.

---

## Steam: Fahrenheit x 2 (Fx2)

The steam generator uses a different encoding: **Fahrenheit multiplied by 2** (Fx2). This is because the steam hardware was designed for the North American market where Fahrenheit is standard.

### Conversion Formula

```
Fx2 = ((Cx2 * 9) / 5) + 64
```

Breaking this down:

- `Cx2 * 9 / 5` converts the doubled-Celsius scale factor to doubled-Fahrenheit.
- `+ 64` adds the Fahrenheit offset (32 degrees F, doubled = 64).

### Steam Temperature Range

| Parameter      | Cx2 | Actual        |
| -------------- | --- | ------------- |
| Steam min temp | 48  | 24 C / 75.2 F |
| Steam max temp | --  | 125 F         |

---

## Conversion Functions

Independent reference implementations for temperature conversions used throughout the system.

### Celsius to Fahrenheit

```python
def celsius_to_fahrenheit(celsius):
    """Standard Celsius to Fahrenheit conversion."""
    return (celsius * 9.0 / 5.0) + 32.0
```

### Fahrenheit to Celsius

```python
def fahrenheit_to_celsius(fahrenheit):
    """Standard Fahrenheit to Celsius conversion."""
    return (fahrenheit - 32.0) * 5.0 / 9.0
```

### Cx2 to Fahrenheit

```python
def cx2_to_fahrenheit(cx2_value):
    """Convert a Cx2 encoded value to degrees Fahrenheit."""
    celsius = cx2_value / 2.0
    return (celsius * 9.0 / 5.0) + 32.0
```

### Fahrenheit to Cx2

```python
def fahrenheit_to_cx2(fahrenheit):
    """Convert degrees Fahrenheit to Cx2 encoding."""
    celsius = (fahrenheit - 32.0) * 5.0 / 9.0
    return int(round(celsius * 2.0))
```

### Cx2 to Fx2

```python
def cx2_to_fx2(cx2_value):
    """Convert Cx2 (Celsius x 2) to Fx2 (Fahrenheit x 2)."""
    return ((cx2_value * 9) // 5) + 64
```

### Fx2 to Cx2

```python
def fx2_to_cx2(fx2_value):
    """Convert Fx2 (Fahrenheit x 2) to Cx2 (Celsius x 2)."""
    return ((fx2_value - 64) * 5) // 9
```

### Quick Reference Table

| Cx2 | Celsius | Fahrenheit | Fx2 |
| --- | ------- | ---------- | --- |
| 60  | 30.0    | 86.0       | 172 |
| 70  | 35.0    | 95.0       | 190 |
| 75  | 37.5    | 99.5       | 199 |
| 80  | 40.0    | 104.0      | 208 |
| 86  | 43.0    | 109.4      | 218 |
| 90  | 45.0    | 113.0      | 226 |
| 98  | 49.0    | 120.2      | 240 |

---

## CGI Temperature Parameters

The web interface uses two different representations depending on the endpoint:

### quick_shower.cgi

Accepts temperature as a **Celsius float** (human-readable):

```
GET /quick_shower.cgi?valve=1&temp=40.5&outlet=1
```

The controller converts the float to Cx2 internally (40.5 C becomes Cx2 = 81).

### save_variable.cgi

Uses **raw Cx2 values** directly:

```
GET /save_variable.cgi?idx=<variable_id>&val=80
```

Here `val=80` means 40.0 C in Cx2 encoding. No conversion is performed by the CGI handler.

When working with `save_variable.cgi`, you must encode/decode Cx2 yourself.

---

## Calibration Impact

Each valve can be calibrated to correct for installation-specific hot and cold supply temperatures. Calibration offsets are stored in the valve's EEPROM and affect how the valve interprets setpoints.

If calibration values are incorrect, the actual delivered water temperature will differ from the setpoint. See the [valve calibration section](../devices/valve-control.md#calibration-system) for calibration procedures.

---

## Safety Warning

**Always verify actual water temperature with a physical thermometer after any calibration change or system modification.** The Cx2 value displayed in the UI or returned by the API represents the valve's thermistor reading, not an independent measurement. Thermistor drift, calibration errors, or plumbing issues can cause the actual water temperature to differ from the reported value. Scalding can occur at temperatures above 43 C (109 F).
