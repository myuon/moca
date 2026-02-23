fun take_dyn(d: dyn) -> string {
    return debug(d);
}

// Redundant: concrete types passed to dyn parameter
take_dyn(42 as dyn);
take_dyn("hello" as dyn);
take_dyn(true as dyn);
