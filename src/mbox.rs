//! Video core mailbox interface.

#![allow(dead_code)]

use core::cmp::max;
use core::hint::spin_loop;
use core::mem::{align_of, size_of, size_of_val};
use core::slice::from_raw_parts as slice_from_raw_parts;

use crate::{cleanup_cache, invalidate_cache};

/// Assembles a buffer with the properties specified on input, sends it through
/// the Mailbox interface, and populates the outputs with the returned
/// properties.
///
/// Panics if the video core fails to parse the buffer, does not know some of
/// the properties, there isn't enough capacity to store a response property's
/// payload, or the alignment requirements of any of the payloads cannot be
/// fulfilled.
#[macro_export]
macro_rules! mbox {
    {msg = $msg:ident , $tag:tt : $input:expr => _ $(, $($tail:tt)*)?} => {{
        let prop = $crate::mbox::Property::new($tag, $input);
        $msg.add_property(&prop);
        $crate::mbox! {msg = $msg $(,$($tail)*)?};
        prop.nop(());
    }};
    {msg = $msg:ident , $tag:tt : _ => $output:expr $(, $($tail:tt)*)?} => {{
        let mut prop = $crate::mbox::Property::new($tag, ());
        $msg.add_property(&prop);
        $crate::mbox! {msg = $msg $(,$($tail)*)?};
        prop = $msg.find_property($tag);
        $output = prop.payload();
    }};
    {msg = $msg:ident , $tag:tt : $input:expr => $output:expr $(, $($tail:tt)*)?} => {{
        let mut prop = $crate::mbox::Property::new($tag, $input);
        $msg.add_property(&prop);
        $crate::mbox! {msg = $msg $(,$($tail)*)?};
        prop = $msg.find_property($tag);
        $output = prop.payload();
    }};
    {msg = $msg:ident} => {{
        $crate::mbox::Mailbox.exchange(&mut $msg);
    }};
    {$($tag:tt : $input:tt => $output:tt),* $(,)?} => {{
        let mut msg = $crate::mbox::Message::new();
        $crate::mbox! {msg = msg, $($tag: $input => $output),*};
    }};
}

/// Base address of the video core mailbox registers.
const BASE: usize = 0x107C013880;
/// Inbox data register.
const INBOX_DATA: *const u32 = BASE as _;
/// Inbox status register.
const INBOX_STATUS: *const u32 = (BASE + 0x18) as _;
/// Outbox data register.
const OUTBOX_DATA: *mut u32 = (BASE + 0x20) as _;
/// Outbox status register.
const OUTBOX_STATUS: *const u32 = (BASE + 0x38) as _;
/// Mailbox full status value.
const FULL_STATUS: u32 = 0x80000000;
/// Mailbox empty status value.
const EMPTY_STATUS: u32 = 0x40000000;
/// Request code.
const REQUEST_CODE: u32 = 0x0;
/// Success code.
const SUCCESS_CODE: u32 = 0x80000000;
/// End tag.
const END_TAG: u32 = 0x0;
/// Message buffer size.
const BUF_SIZE: usize = 0x100;

/// Mailbox interface driver.
pub struct Mailbox;

/// Message buffer.
#[repr(align(64), C)] // Align to a cache line.
pub union Message
{
    /// Message header.
    header: MessageHeader,
    /// Byte view.
    byte_view: [u8; BUF_SIZE],
    /// Unsigned int 32 view.
    int_view: [u32; BUF_SIZE / 4],
}

/// Message property.
#[repr(C)]
#[derive(Clone, Copy)]
pub union Property<I: Copy, O: Copy>
{
    /// Property header.
    header: PropertyHeader,
    /// Input data.
    input: PropertyData<I>,
    /// Output data.
    output: PropertyData<O>,
}

/// Property data.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct PropertyData<T: Copy>
{
    /// Property header.
    header: PropertyHeader,
    /// Property payload.
    payload: T,
}

/// Message header.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct MessageHeader
{
    /// Message buffer size.
    size: u32,
    /// Message type code.
    code: u32,
    /// First tag.
    tag: u32,
}

/// Property header.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct PropertyHeader
{
    /// Property tag.
    tag: u32,
    /// Allocated buffer size.
    buf_size: u32,
    /// Response size.
    resp_size: u32,
}

impl Mailbox
{
    /// Delivers the request and waits for a response.
    ///
    /// * `msg`: Message with the request on input and response on output.
    ///
    /// Panics if the message is not a request on input or a success response on
    /// output.
    #[track_caller]
    pub fn exchange(&mut self, msg: &mut Message)
    {
        let code = unsafe { msg.header.code };
        assert!(code == REQUEST_CODE,
                "Attempted to deliver a message to the firmware that is not a request");
        let buf = unsafe { &mut msg.byte_view };
        while unsafe { OUTBOX_STATUS.read_volatile() } & FULL_STATUS != 0 {
            spin_loop()
        }
        let data = buf.as_ptr() as usize as u32 | 0xC0000008;
        cleanup_cache(buf);
        unsafe { OUTBOX_DATA.write_volatile(data) };
        while unsafe { INBOX_STATUS.read_volatile() } & EMPTY_STATUS != 0 {
            spin_loop()
        }
        unsafe { INBOX_DATA.read_volatile() }; // Don't care about this value, just reading it to empty the inbox.
        invalidate_cache(buf);
        let code = unsafe { msg.header.code };
        assert!(code == SUCCESS_CODE,
                "Firmware reply contains an unexpected code: 0x{code:X}");
    }
}

impl Message
{
    /// Creates and initializes a new message.
    ///
    /// Returns the newly created message.
    pub fn new() -> Self
    {
        let header = MessageHeader { size: BUF_SIZE as _,
                                     code: REQUEST_CODE,
                                     tag: END_TAG };
        Self { header }
    }

    /// Adds a property to the message.
    ///
    /// * `prop`: Property to add.
    ///
    /// Panics if pushing the property would overflow the message or a property
    /// with the same tag already exists in the message.
    #[track_caller]
    pub fn add_property<I: Copy, O: Copy>(&mut self, prop: &Property<I, O>)
    {
        // Find the end tag.
        let mut idx = 8;
        while unsafe { self.int_view[idx / 4] } != END_TAG {
            assert!(unsafe { self.int_view[idx / 4] } != prop.tag(),
                    "Duplicate property tag: 0x{:X}",
                    prop.tag());
            idx += ((unsafe { self.int_view[idx / 4 + 1] } as usize + 0x3) & !0x3) + 12;
        }
        let size = size_of_val(prop);
        assert!(idx + size + 4 <= BUF_SIZE,
                "Adding this property would overflow the message");
        // Copy the property.
        unsafe { self.byte_view[idx .. idx + size].copy_from_slice(prop.bytes()) };
        idx += size;
        unsafe { self.int_view[(idx + 3) / 4] = END_TAG };
    }

    // Finds a property by its tag.
    //
    // * `tag`: Property tag to search for.
    //
    // Returns the property.
    //
    // Panics if there's no property with the specified tag in the message.
    #[track_caller]
    pub fn find_property<I: Copy, O: Copy>(&mut self, tag: u32) -> Property<I, O>
    {
        let code = unsafe { self.header.code };
        assert!(
                code == SUCCESS_CODE,
                "Message was either not parsed by the firmware or it returned an error (code:
0x{code:X})"
        );
        let mut idx = 8;
        while unsafe { self.int_view[idx / 4] } != tag {
            assert!(unsafe { self.int_view[idx / 4] } != END_TAG,
                    "Tag 0x{tag:X} not found in message");
            idx += ((unsafe { self.int_view[idx / 4 + 1] } as usize + 0x3) & !0x3) + 12;
        }
        Property::from_bytes(unsafe { &self.byte_view[idx .. idx + size_of::<Property<I, O>>()] })
    }
}

impl<I: Copy, O: Copy> Property<I, O>
{
    /// Creates and nitializes a new property.
    ///
    /// * `tag`: Property tag.
    /// * `payload`: Property payload.
    ///
    /// Returns the newly created property.
    ///
    /// Panics if the alignment of either request or response types is not
    /// supported.
    #[track_caller]
    pub fn new(tag: u32, payload: I) -> Self
    {
        let align = align_of::<Self>();
        assert!(align == 4, "Property has an unsupported alignment");
        let size = max(size_of::<I>(), size_of::<O>());
        let header = PropertyHeader { tag,
                                      buf_size: size as _,
                                      resp_size: 0 };
        let input = PropertyData { header, payload };
        Self { input }
    }

    // initializes a new property from its byte representation.
    //
    // * `bytes`: Byte representation of the property.
    //
    // Returns the newly created property.
    //
    // Panics if the alignment of either the request or response types is not
    // supported or the length of the slice doesn't match the size of the property
    // being created. #[track_caller]
    fn from_bytes(bytes: &[u8]) -> Self
    {
        let align = align_of::<Self>();
        assert!(align == 4, "Property has an unsupported alignment");
        let size = size_of::<Self>();
        assert!(bytes.len() == size, "Slice size doesn't match the property's size");
        unsafe { *(bytes.as_ptr() as *const Self) }
    }

    /// Returns this property's tag.
    fn tag(&self) -> u32
    {
        unsafe { self.header.tag }
    }

    // Returns this
    // property's payload.
    //
    // Panics if this is not a response.
    // #[track_caller]
    pub fn payload(&self) -> O
    {
        let resp_size = unsafe { self.header.resp_size };
        let tag = unsafe { self.header.tag };
        assert!(resp_size & 0x80000000 != 0,
                "No response for property with tag 0x{tag:X}");
        let tag = unsafe { self.header.tag };
        let resp_size = resp_size & !0x80000000;
        let buf_size = unsafe { self.header.buf_size };
        assert!(
                resp_size <= buf_size,
                "Response to tag 0x{tag:X} is truncated (capacity: {buf_size}, size:
{resp_size})"
        );
        unsafe { self.output.payload }
    }

    /// Returns a byte representation of this property.
    fn bytes(&self) -> &[u8]
    {
        unsafe { slice_from_raw_parts(self as *const Self as *const u8, size_of::<Self>()) }
    }

    /// Little hack to make type inference work in the macro when the user does
    /// not specify an output binding.
    pub fn nop(&self, output: O) -> O
    {
        output
    }
}
