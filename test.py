import requests
import time


URL = "http://127.0.0.1:8001/"

times = 0

# setup
r = requests.post(URL + "setup", json={})
if r.status_code == 200:
    times += 1
    print(times, r.text)
else:
    print("ERROR")
    exit(1)

# register
for _ in range(2):
    r = requests.post(URL + "register", json={"pubkey": "00", "psk": "00"})
    if r.status_code == 200:
        times += 1
        print(times, r.text)
    else:
        print("ERROR REGISTER")
        exit(1)

print("Waiting register all ok")
time.sleep(10)

# setup
r = requests.post(URL + "deposit", json={"to": "0", "amount": "10000", "psk": "00"})
if r.status_code == 200:
    times += 1
    print(times, r.text)
else:
    print("ERROR DEPOSIT")
    exit(1)

print("Waiting deposit is ok")
time.sleep(15)


uses = ["0", "1"]

t_amount = 10000
s_amount = 0

while True:
    times += 1
    time.sleep(1)

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
