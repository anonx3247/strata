# A Python program calling what geo.strata exports (`derive Python for f`),
# through the module `strata build --lib` wrote beside the library.
import geo
from geo import Point, Circle, Square, Dot, ByZero, StrataError

print(geo.add(2, 40))
print(geo.middle(Point(0.0, 0.0), Point(2.0, 4.0)))
print(geo.area(Circle(Point(0.0, 0.0), 2.0)), geo.area(Square(3.0)), geo.area(Dot()))
print(geo.repeat("ab", 3), geo.repeat("c"))
print(geo.divide(7, 2))
print(geo.louder("Quiet"))
print(geo.hello.__annotations__, geo.repeat.__annotations__["n"])
# errors come back as StrataError, never a crash
for call in [
    lambda: geo.divide(1, 0),
    lambda: geo.repeat("x", 0),
    lambda: geo.add("two", 2),
    lambda: geo.add(2**60, 1),
    lambda: geo.louder("Medium"),
    lambda: geo._call("add_json", {"a": 1}),
    lambda: geo._call("add_json", {"a": 1, "b": 2, "c": 3}),
]:
    try:
        print("no error:", call())
    except StrataError as e:
        print(e.kind, "|", e.message, "|", e.value)
# the raw export: bad JSON, not an object
import ctypes
geo._lib.add_json.argtypes = [ctypes.c_char_p]
geo._lib.add_json.restype = ctypes.c_void_p
for text in [b"{", b"[1, 2]"]:
    p = geo._lib.add_json(text)
    print(ctypes.string_at(p).decode())
    geo._lib.strata_string_free(p)
# (what Strata prints is written when the process ends)
nothing = geo.hello()
print(nothing)
