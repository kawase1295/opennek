import { expect, test } from 'vitest'
import { toPortList } from '../src/serial'

// Entries as SerialPort.list() returns them on each OS
const espressif = { manufacturer: 'Espressif', serialNumber: 'D8:3B:DA:4A:1C:20', vendorId: '303a', productId: '1001' }

test('Linux lists USB serial devices by their /dev/serial/by-id name and hides the built-in ttyS ports', () => {
  expect(
    toPortList(
      [
        { path: '/dev/ttyS0' },
        { path: '/dev/ttyACM0', pnpId: 'usb-Espressif_USB_JTAG_serial_debug_unit_D8:3B:DA:4A:1C:20-if00', ...espressif },
        { path: '/dev/ttyUSB0' },
      ],
      'linux',
    ),
  ).toEqual([
    { path: '/dev/ttyACM0', label: 'usb-Espressif_USB_JTAG_serial_debug_unit_D8:3B:DA:4A:1C:20-if00' },
    { path: '/dev/ttyUSB0', label: 'ttyUSB0' },
  ])
})

test('Windows lists every COM port with its friendly name', () => {
  expect(
    toPortList(
      [
        { path: 'COM4', friendlyName: 'USB Serial Device (COM4)', ...espressif },
        { path: 'COM1', friendlyName: 'Communications Port (COM1)' },
        { path: 'COM10' },
      ],
      'win32',
    ),
  ).toEqual([
    { path: 'COM1', label: 'Communications Port (COM1)' },
    { path: 'COM4', label: 'USB Serial Device (COM4)' },
    { path: 'COM10', label: 'COM10' },
  ])
})

test('macOS lists USB devices on their callout (cu) node', () => {
  expect(
    toPortList(
      [
        { path: '/dev/tty.Bluetooth-Incoming-Port' },
        { path: '/dev/tty.usbmodem1101', ...espressif },
      ],
      'darwin',
    ),
  ).toEqual([{ path: '/dev/cu.usbmodem1101', label: 'Espressif D8:3B:DA:4A:1C:20' }])
})
