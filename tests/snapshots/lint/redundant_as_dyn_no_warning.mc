fun take_dyn(d: dyn) -> string {
    return debug(d);
}

// Not redundant: implicit coercion (no as dyn in source)
take_dyn(42);

// Not redundant: variable already has dyn type
let _d = 42 as dyn;
take_dyn(_d);
