import os


def inc(x):
    return x + 1


def die(code):
    os._exit(code)


def nan():
    return float("nan")


def big():
    return 2 ** 60


def plain(n):
    return {"x": n}


def wrong():
    return {"x": "not a number"}


def negative():
    return -5


def noisy():
    print("printed by Python")
    return None


def unknown_value():
    return {1, 2}
