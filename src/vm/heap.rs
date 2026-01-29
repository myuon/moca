use std::collections::HashMap;
use std::fmt;

use super::Value;

/// Type ID for heap objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    String,
    Array,
    Object,
    /// Generic slot-based heap object (for new array implementation)
    Slots,
    /// Dynamic array with capacity management
    Vector,
}

/// Header for all heap objects.
#[derive(Debug)]
pub struct ObjectHeader {
    pub obj_type: ObjectType,
    pub marked: bool,
}

/// A heap-allocated string.
#[derive(Debug)]
pub struct MocaString {
    pub header: ObjectHeader,
    pub value: String,
}

/// A heap-allocated array.
#[derive(Debug)]
pub struct MocaArray {
    pub header: ObjectHeader,
    pub elements: Vec<Value>,
}

/// A heap-allocated object (key-value map).
#[derive(Debug)]
pub struct MocaObject {
    pub header: ObjectHeader,
    pub fields: HashMap<String, Value>,
    /// Shape ID for inline caching (objects with same fields have same shape)
    pub shape_id: u32,
}

/// A generic slot-based heap object.
/// Used for the new array implementation where:
/// - slot 0: length (as Value::I64)
/// - slot 1..n: elements
#[derive(Debug)]
pub struct MocaSlots {
    pub header: ObjectHeader,
    pub slots: Vec<Value>,
}

/// A dynamic array (Vector) with capacity management.
/// Layout:
/// - ptr: GcRef to a Slots object containing the data
/// - len: current number of elements (as i64)
/// - cap: capacity (as i64)
#[derive(Debug)]
pub struct MocaVector {
    pub header: ObjectHeader,
    /// Reference to the data storage (a Slots object), or None if empty
    pub ptr: Option<GcRef>,
    /// Current number of elements
    pub len: i64,
    /// Current capacity
    pub cap: i64,
}

/// A heap object can be any of the heap-allocated types.
pub enum HeapObject {
    String(MocaString),
    Array(MocaArray),
    Object(MocaObject),
    Slots(MocaSlots),
    Vector(MocaVector),
}

impl HeapObject {
    pub fn obj_type(&self) -> ObjectType {
        match self {
            HeapObject::String(_) => ObjectType::String,
            HeapObject::Array(_) => ObjectType::Array,
            HeapObject::Object(_) => ObjectType::Object,
            HeapObject::Slots(_) => ObjectType::Slots,
            HeapObject::Vector(_) => ObjectType::Vector,
        }
    }

    pub fn header(&self) -> &ObjectHeader {
        match self {
            HeapObject::String(s) => &s.header,
            HeapObject::Array(a) => &a.header,
            HeapObject::Object(o) => &o.header,
            HeapObject::Slots(s) => &s.header,
            HeapObject::Vector(v) => &v.header,
        }
    }

    pub fn header_mut(&mut self) -> &mut ObjectHeader {
        match self {
            HeapObject::String(s) => &mut s.header,
            HeapObject::Array(a) => &mut a.header,
            HeapObject::Object(o) => &mut o.header,
            HeapObject::Slots(s) => &mut s.header,
            HeapObject::Vector(v) => &mut v.header,
        }
    }

    pub fn as_string(&self) -> Option<&MocaString> {
        match self {
            HeapObject::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&MocaArray> {
        match self {
            HeapObject::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut MocaArray> {
        match self {
            HeapObject::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&MocaObject> {
        match self {
            HeapObject::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut MocaObject> {
        match self {
            HeapObject::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_slots(&self) -> Option<&MocaSlots> {
        match self {
            HeapObject::Slots(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_slots_mut(&mut self) -> Option<&mut MocaSlots> {
        match self {
            HeapObject::Slots(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_vector(&self) -> Option<&MocaVector> {
        match self {
            HeapObject::Vector(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_vector_mut(&mut self) -> Option<&mut MocaVector> {
        match self {
            HeapObject::Vector(v) => Some(v),
            _ => None,
        }
    }

    /// Get all Value references in this object for GC tracing.
    pub fn trace(&self) -> Vec<GcRef> {
        match self {
            HeapObject::String(_) => vec![],
            HeapObject::Array(arr) => arr.elements.iter().filter_map(|v| v.as_ref()).collect(),
            HeapObject::Object(obj) => obj.fields.values().filter_map(|v| v.as_ref()).collect(),
            // For Slots, skip slot 0 (length) and trace the rest
            HeapObject::Slots(slots) => slots.slots.iter().skip(1).filter_map(|v| v.as_ref()).collect(),
            // For Vector, trace the data pointer (the Slots object will trace its contents)
            HeapObject::Vector(vec) => vec.ptr.into_iter().collect(),
        }
    }
}

impl fmt::Debug for HeapObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapObject::String(s) => write!(f, "String({:?})", s.value),
            HeapObject::Array(a) => write!(f, "Array({:?})", a.elements),
            HeapObject::Object(o) => write!(f, "Object({:?})", o.fields),
            HeapObject::Slots(s) => write!(f, "Slots({:?})", s.slots),
            HeapObject::Vector(v) => write!(f, "Vector(ptr={:?}, len={}, cap={})", v.ptr, v.len, v.cap),
        }
    }
}

/// A reference to a heap object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcRef {
    pub index: usize,
}

/// The garbage-collected heap.
pub struct Heap {
    objects: Vec<Option<HeapObject>>,
    free_list: Vec<usize>,
    bytes_allocated: usize,
    gc_threshold: usize,
    /// Counter for generating unique shape IDs
    next_shape_id: u32,
    /// Cache of shape signatures to shape IDs
    shape_cache: HashMap<Vec<String>, u32>,
    /// Hard limit on heap size (None = unlimited)
    heap_limit: Option<usize>,
    /// Whether GC is enabled
    gc_enabled: bool,
}

impl Heap {
    pub fn new() -> Self {
        Self::new_with_config(None, true)
    }

    /// Create a new heap with custom configuration.
    ///
    /// # Arguments
    /// * `heap_limit` - Hard limit on heap size in bytes (None = unlimited)
    /// * `gc_enabled` - Whether GC is enabled
    pub fn new_with_config(heap_limit: Option<usize>, gc_enabled: bool) -> Self {
        Self {
            objects: Vec::new(),
            free_list: Vec::new(),
            bytes_allocated: 0,
            gc_threshold: 1024 * 1024, // 1MB initial threshold
            next_shape_id: 1,
            shape_cache: HashMap::new(),
            heap_limit,
            gc_enabled,
        }
    }

    /// Compute or retrieve a shape ID for a given set of field names.
    fn get_shape_id(&mut self, field_names: &[String]) -> u32 {
        let mut sorted_names: Vec<String> = field_names.to_vec();
        sorted_names.sort();

        if let Some(&id) = self.shape_cache.get(&sorted_names) {
            id
        } else {
            let id = self.next_shape_id;
            self.next_shape_id += 1;
            self.shape_cache.insert(sorted_names, id);
            id
        }
    }

    /// Check if allocation would exceed heap limit.
    fn check_heap_limit(&self, additional_size: usize) -> Result<(), String> {
        if let Some(limit) = self.heap_limit {
            let new_total = self.bytes_allocated + additional_size;
            if new_total > limit {
                return Err(format!(
                    "runtime error: heap limit exceeded (allocated: {} bytes, limit: {} bytes)",
                    new_total, limit
                ));
            }
        }
        Ok(())
    }

    /// Allocate a new string on the heap.
    pub fn alloc_string(&mut self, value: String) -> Result<GcRef, String> {
        let size = std::mem::size_of::<MocaString>() + value.len();
        self.check_heap_limit(size)?;
        self.bytes_allocated += size;

        let obj = HeapObject::String(MocaString {
            header: ObjectHeader {
                obj_type: ObjectType::String,
                marked: false,
            },
            value,
        });
        Ok(self.alloc_object(obj))
    }

    /// Allocate a new array on the heap.
    pub fn alloc_array(&mut self, elements: Vec<Value>) -> Result<GcRef, String> {
        let size = std::mem::size_of::<MocaArray>() + elements.len() * std::mem::size_of::<Value>();
        self.check_heap_limit(size)?;
        self.bytes_allocated += size;

        let obj = HeapObject::Array(MocaArray {
            header: ObjectHeader {
                obj_type: ObjectType::Array,
                marked: false,
            },
            elements,
        });
        Ok(self.alloc_object(obj))
    }

    /// Allocate a new object on the heap.
    pub fn alloc_object_map(&mut self, fields: HashMap<String, Value>) -> Result<GcRef, String> {
        let size = std::mem::size_of::<MocaObject>()
            + fields.len() * (std::mem::size_of::<String>() + std::mem::size_of::<Value>());
        self.check_heap_limit(size)?;
        self.bytes_allocated += size;

        // Compute shape ID based on field names
        let field_names: Vec<String> = fields.keys().cloned().collect();
        let shape_id = self.get_shape_id(&field_names);

        let obj = HeapObject::Object(MocaObject {
            header: ObjectHeader {
                obj_type: ObjectType::Object,
                marked: false,
            },
            fields,
            shape_id,
        });
        Ok(self.alloc_object(obj))
    }

    /// Allocate a new slot-based heap object.
    /// This is used for the new array implementation where slots[0] is length.
    pub fn alloc_slots(&mut self, slots: Vec<Value>) -> Result<GcRef, String> {
        let size = std::mem::size_of::<MocaSlots>() + slots.len() * std::mem::size_of::<Value>();
        self.check_heap_limit(size)?;
        self.bytes_allocated += size;

        let obj = HeapObject::Slots(MocaSlots {
            header: ObjectHeader {
                obj_type: ObjectType::Slots,
                marked: false,
            },
            slots,
        });
        Ok(self.alloc_object(obj))
    }

    /// Allocate a new Vector object.
    /// The Vector contains a pointer to data (Slots), length, and capacity.
    pub fn alloc_vector(&mut self, ptr: Option<GcRef>, len: i64, cap: i64) -> Result<GcRef, String> {
        let size = std::mem::size_of::<MocaVector>();
        self.check_heap_limit(size)?;
        self.bytes_allocated += size;

        let obj = HeapObject::Vector(MocaVector {
            header: ObjectHeader {
                obj_type: ObjectType::Vector,
                marked: false,
            },
            ptr,
            len,
            cap,
        });
        Ok(self.alloc_object(obj))
    }

    fn alloc_object(&mut self, obj: HeapObject) -> GcRef {
        if let Some(index) = self.free_list.pop() {
            self.objects[index] = Some(obj);
            GcRef { index }
        } else {
            let index = self.objects.len();
            self.objects.push(Some(obj));
            GcRef { index }
        }
    }

    /// Get an object by reference.
    pub fn get(&self, r: GcRef) -> Option<&HeapObject> {
        self.objects.get(r.index).and_then(|o| o.as_ref())
    }

    /// Get a mutable reference to an object.
    pub fn get_mut(&mut self, r: GcRef) -> Option<&mut HeapObject> {
        self.objects.get_mut(r.index).and_then(|o| o.as_mut())
    }

    /// Check if GC should be triggered.
    pub fn should_gc(&self) -> bool {
        self.gc_enabled && self.bytes_allocated >= self.gc_threshold
    }

    /// Get the number of bytes currently allocated.
    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated
    }

    /// Mark phase: mark all reachable objects.
    pub fn mark(&mut self, roots: &[Value]) {
        // Collect all root references
        let mut worklist: Vec<GcRef> = roots.iter().filter_map(|v| v.as_ref()).collect();

        // Mark and trace
        while let Some(r) = worklist.pop() {
            if let Some(obj) = self.objects.get_mut(r.index).and_then(|o| o.as_mut())
                && !obj.header().marked
            {
                obj.header_mut().marked = true;
                // Trace children
                worklist.extend(obj.trace());
            }
        }
    }

    /// Sweep phase: free all unmarked objects.
    pub fn sweep(&mut self) {
        for i in 0..self.objects.len() {
            if let Some(obj) = &mut self.objects[i] {
                if obj.header().marked {
                    // Reset mark for next GC cycle
                    obj.header_mut().marked = false;
                } else {
                    // Free unmarked object
                    self.objects[i] = None;
                    self.free_list.push(i);
                }
            }
        }

        // Recalculate bytes allocated
        self.bytes_allocated = 0;
        for o in self.objects.iter().flatten() {
            self.bytes_allocated += match o {
                HeapObject::String(s) => std::mem::size_of::<MocaString>() + s.value.len(),
                HeapObject::Array(a) => {
                    std::mem::size_of::<MocaArray>()
                        + a.elements.len() * std::mem::size_of::<Value>()
                }
                HeapObject::Object(o) => {
                    std::mem::size_of::<MocaObject>()
                        + o.fields.len()
                            * (std::mem::size_of::<String>() + std::mem::size_of::<Value>())
                }
                HeapObject::Slots(s) => {
                    std::mem::size_of::<MocaSlots>()
                        + s.slots.len() * std::mem::size_of::<Value>()
                }
                HeapObject::Vector(_) => std::mem::size_of::<MocaVector>(),
            };
        }

        // Adjust threshold
        self.gc_threshold = (self.bytes_allocated * 2).max(1024 * 1024);
    }

    /// Perform a full garbage collection cycle.
    pub fn collect(&mut self, roots: &[Value]) {
        self.mark(roots);
        self.sweep();
    }

    /// Get count of live objects.
    pub fn object_count(&self) -> usize {
        self.objects.iter().filter(|o| o.is_some()).count()
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_string() {
        let mut heap = Heap::new();
        let r = heap.alloc_string("hello".to_string()).unwrap();
        let obj = heap.get(r).unwrap();
        assert_eq!(obj.as_string().unwrap().value, "hello");
    }

    #[test]
    fn test_alloc_array() {
        let mut heap = Heap::new();
        let r = heap
            .alloc_array(vec![Value::I64(1), Value::I64(2), Value::I64(3)])
            .unwrap();
        let obj = heap.get(r).unwrap();
        assert_eq!(obj.as_array().unwrap().elements.len(), 3);
    }

    #[test]
    fn test_gc_collects_unreachable() {
        let mut heap = Heap::new();

        // Allocate some objects
        let _r1 = heap.alloc_string("garbage".to_string()).unwrap();
        let r2 = heap.alloc_string("keep".to_string()).unwrap();

        assert_eq!(heap.object_count(), 2);

        // Only r2 is in roots
        heap.collect(&[Value::Ref(r2)]);

        assert_eq!(heap.object_count(), 1);
        assert!(heap.get(r2).is_some());
    }

    #[test]
    fn test_gc_traces_arrays() {
        let mut heap = Heap::new();

        // Create a string
        let str_ref = heap.alloc_string("inside array".to_string()).unwrap();

        // Create an array containing the string
        let arr_ref = heap.alloc_array(vec![Value::Ref(str_ref)]).unwrap();

        assert_eq!(heap.object_count(), 2);

        // Only array is in roots, but string should be kept via tracing
        heap.collect(&[Value::Ref(arr_ref)]);

        assert_eq!(heap.object_count(), 2);
        assert!(heap.get(str_ref).is_some());
        assert!(heap.get(arr_ref).is_some());
    }
}
