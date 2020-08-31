import requests
import time


URL = "http://127.0.0.1:8001/"

times = 0

# setup
print("Start setup...")
r = requests.post(URL + "setup", json={})
if r.status_code == 200:
    times += 1
    print(times, r.text)
else:
    print("ERROR")
    exit(1)
print("Setup ok")

# register
print("Start register test, register two account, maybe 10s")
for _ in range(2):
    r = requests.post(URL + "register", json={"pubkey": "00", "psk": "00"})
    if r.status_code == 200:
        times += 1
        print(times, r.text)
    else:
        print("ERROR REGISTER")
        exit(1)

print("Waiting register onchain...")
time.sleep(10)

# deposit
print("Start deposit test, maybe need 15s")
r = requests.post(URL + "deposit", json={"to": "0", "amount": "10000", "psk": "00"})
if r.status_code == 200:
    times += 1
    print(times, r.text)
else:
    print("ERROR DEPOSIT")
    exit(1)

print("Waiting deposit onchain...")
time.sleep(15)

# withdraw
print("Start withdraw test, maybe need 15s")
r = requests.post(URL + "withdraw", json={"from": "0", "amount": "10", "psk": "00"})
if r.status_code == 200:
    times += 1
    print(times, r.text)
else:
    print("ERROR DEPOSIT")
    exit(1)

print("Waiting withdraw onchain...")
time.sleep(15)

print("Start transfer test, loop run in 0.4s new a transfer (CTRL+c to kill)...")
uses = ["0", "1"]

t_amount = 10000
s_amount = 0

while True:
    times += 1
    time.sleep(0.4)

    t_from = "0"
    t_to = "1"
    amount = 9

    data = {
        "from": t_from,
        "to": t_to,
        "amount": str(amount),
        "psk": "00",
    }

    r = requests.post(URL + "transfer", json=data)
    if r.status_code == 200:
        t_amount -= amount
        s_amount += amount
        print(times, r.text)
    else:
        print("ERROR TRANSFER")
        exit(1)
