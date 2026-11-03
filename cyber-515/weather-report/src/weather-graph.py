#Author: Aaron Williams-Breth
# Use 20 years of temperature data

import requests
import pandas as pd
import matplotlib.pyplot as plt

weather_url = "https://archive-api.open-meteo.com/v1/archive"
# Weather from Akron Ohio (REPRESENT) from the year I was born to Today placed into a dictionary to interact with the API
params = {
    "latitude": 41.08,
    "longitude": -81.52,
    "start_date": "2004-07-27",
    "end_date": "2026-11-01",
    "hourly": "temperature_2m",
    "temperature_unit": "fahrenheit",
    "timezone": "America/New_York",
}

# Get the data, with a timeout ofc 
data = requests.get(weather_url, params=params, timeout=300).json()["hourly"]

# Pandas, numbers then date and time
temp = pd.Series(data["temperature_2m"], index=pd.to_datetime(data["time"]))


# Group the hourly readings by day and average each day
daily_mean = temp.resample("D").mean()
# Group the hourly readings by day and find each day's standard deviation
daily_std = temp.resample("D").std()
# Get every 3 hour interval (0,3,6,9...,21) cause the assignment asks for 2-4 hour intervals
raw = temp[temp.index.hour % 3 == 0]


# Gray dots: raw readings every 3 hours
plt.scatter(raw.index, raw, s=0.5, color="gray", label="Every 3 hours")

# Blue line: daily mean
plt.plot(daily_mean.index, daily_mean, color="blue", linewidth=0.3, label="Daily mean")

# Light blue band: daily mean plus or minus 1 standard deviation
plt.fill_between(daily_mean.index, daily_mean - daily_std, daily_mean + daily_std, color="blue", alpha=0.3, label="Mean ± 1 std. dev.")

# Labels, title and legend
plt.xlabel("Date")
plt.ylabel("Temperature (F)")
plt.title("Hourly Temperature in Akron, OH, July 2004 - November 2026")
plt.legend(markerscale=10)

# Save the picture for the LaTeX report
plt.savefig("temperature.png", dpi=200)
print("Saved temperature.png")
