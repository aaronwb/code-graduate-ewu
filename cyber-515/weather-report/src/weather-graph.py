#Author: Aaron Williams-Breth
# Use 20 years of temperature data

import requests
import pandas as pd
import matplotlib.pyplot as plt
import matplotlib.dates as mdates

weather_url = "https://archive-api.open-meteo.com/v1/archive"
# Weather from Akron Ohio (@41.0842034,-81.5957888) (REPRESENT) from the year I was born to nov 1st placed into a dictionary to interact with the API
params = {
    "latitude": 41.08,
    "longitude": -81.59,
    "start_date": "2004-07-27",
    "end_date": "2026-11-01",
    "hourly": "temperature_2m",
    "temperature_unit": "fahrenheit",
    "timezone": "America/New_York",
}

# Get the data, with a timeout ofc 
data = requests.get(weather_url, params=params, timeout=300).json()["hourly"]

# pd.to_datetime turns the time text into real dates
temp = pd.Series(data["temperature_2m"], index=pd.to_datetime(data["time"]))


# Group the hourly readings by day and average each day using pandas function
daily_mean = temp.resample("D").mean()
# Group the hourly readings by day and find each day's standard deviation using pandas function
daily_std = temp.resample("D").std()
# Get every 3 hour interval (0,3,6,9...,21) cause the assignment asks for 2-4 hour intervals
raw = temp[temp.index.hour % 3 == 0]


# Make the figure wide so 22 years have room to spread out
plt.figure(figsize=(14, 5))

# Alpha is opacity
# s is size

# Gray dots: raw readings every 3 hours
plt.scatter(raw.index, raw, s=0.3, color="gray", label="Every 3 hours")

# Orange band: daily mean plus or minus 1 standard deviation
plt.fill_between(daily_mean.index, daily_mean - daily_std, daily_mean + daily_std, color="orange", alpha=0.5, label="Mean ± 1 std. dev.")

# Dark blue line: daily mean (very thin so it doesn't cover the band)
plt.plot(daily_mean.index, daily_mean, color="navy", linewidth=0.2, label="Daily mean")

# Labels and title
plt.xlabel("Date")
plt.ylabel("Temperature (°F)")
plt.title("Hourly Temperature in Akron, OH, July 2004 - November 2026")

# Extra room at the bottom for the legend, plus a light grid
plt.ylim(-30, 105)
plt.grid(alpha=0.3)
plt.legend(loc="lower center", ncol=3)

# Save figure
plt.savefig("temperature_full.png", dpi=200, bbox_inches="tight")
print("Saved temperature_full.png")

# ---------------------------------------------

# Second graph: zoom in on just 2025 so individual days are visible
plt.figure(figsize=(14, 5))

# Gray dots: raw readings every 3 hours
plt.scatter(raw.loc["2025"].index, raw.loc["2025"], s=3, color="gray", label="Every 3 hours")

# Orange band: daily mean plus or minus 1 standard deviation
plt.fill_between(daily_mean.loc["2025"].index, daily_mean.loc["2025"] - daily_std.loc["2025"], daily_mean.loc["2025"] + daily_std.loc["2025"], color="orange", alpha=0.5, label="Mean ± 1 std. dev.")

# Dark blue line: daily mean
plt.plot(daily_mean.loc["2025"].index, daily_mean.loc["2025"], color="navy", linewidth=1, label="Daily mean")

# Labels and title
plt.xlabel("Date")
plt.ylabel("Temperature (F)")
plt.title("Hourly Temperature in Akron, OH, 2025")

# Extra room at the bottom for the legend, plus a light grid
plt.ylim(-30, 105)
plt.grid(alpha=0.3)
plt.legend(loc="lower center", ncol=3)

# Save Figure and Print for completion
plt.savefig("temperature_2025.png", dpi=200, bbox_inches="tight")
print("Saved temperature_2025.png")


# -----------------------------------------------------


# Third graph: zoom in on just July 2025 so the daily ups and downs are visible
plt.figure(figsize=(14, 5))

# Pull out just July 2025, otherwise the data sits at the top of the graph for some reason
month_raw = raw.loc["2025-07"]
month_mean = daily_mean.loc["2025-07"].shift(12, freq="h")
month_std = daily_std.loc["2025-07"].shift(12, freq="h")

# Gray dots: raw readings every 3 hours
plt.scatter(month_raw.index, month_raw, s=12, color="gray", label="Every 3 hours")

# Orange band: daily mean plus or minus 1 standard deviation
plt.fill_between(month_mean.index, month_mean - month_std, month_mean + month_std, color="orange", alpha=0.5, label="Mean ± 1 std. dev.")

# Dark blue line: daily mean
plt.plot(month_mean.index, month_mean, color="navy", linewidth=1.5, marker="o", markersize=3, label="Daily mean")

# Labels and title
plt.xlabel("Date")
plt.ylabel("Temperature (F)")
plt.title("Hourly Temperature in Akron, OH, July 2025")

# Limit it to 50 degrees to 105
plt.ylim(50, 105)

# Legend at the top, plus a light grid
plt.grid(alpha=0.3)
plt.legend(loc="lower center", ncol=3)

# Save Figure and Print for completion
plt.savefig("temperature_month.png", dpi=200, bbox_inches="tight")
print("Saved temperature_month.png")
