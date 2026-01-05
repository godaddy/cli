import { Command } from "commander";

// Available actions list
const AVAILABLE_ACTIONS = [
	"location.address.verify",
	"commerce.taxes.calculate",
	"commerce.shipping-rates.calculate",
	"commerce.price-adjustment.apply",
	"commerce.price-adjustment.list",
	"notifications.email.send",
	"commerce.payment.get",
	"commerce.payment.cancel",
	"commerce.payment.refund",
	"commerce.payment.process",
	"commerce.payment.auth",
];

// Action interface definitions (placeholder structure)
interface ActionInterface {
	name: string;
	description: string;
	requestSchema: object;
	responseSchema: object;
	examples?: {
		request: object;
		response: object;
	};
}

export const ACTION_INTERFACES: Record<string, ActionInterface> = {
	"location.address.verify": {
		name: "location.address.verify",
		description: "Verify and standardize a physical address",
		requestSchema: {
			$id: "https://godaddy.com/schema/common/address.v1",
			$schema: "https://json-schema.org/draft/2020-12/schema",
			type: "object",
			title: "Postal Address (Medium-Grained)",
			description:
				"An internationalized postal address. Maps to [AddressValidationMetadata](https://github.com/googlei18n/libaddressinput/wiki/AddressValidationMetadata) from Google's Address Data Service and HTML 5.1 [Autofilling form controls: the autocomplete attribute](https://www.w3.org/TR/html51/sec-forms.html#autofilling-form-controls-the-autocomplete-attribute).",
			properties: {
				addressLine1: {
					type: "string",
					description:
						"The first line of the address. For example, number and street name. For example, `3032 Bunker Hill Lane`. Required for compliance and risk checks. Must contain the full address.",
					maxLength: 300,
				},
				addressLine2: {
					type: "string",
					description:
						"The second line of the address. For example, office suite or apartment number.",
					maxLength: 300,
				},
				addressLine3: {
					type: "string",
					description:
						"The third line of the address, if needed. For example, a street complement for Brazil; direction text, such as `next to Opera House`; or a landmark reference in an Indian address.",
					maxLength: 100,
				},
				adminArea4: {
					description:
						"The neighborhood, ward, or district. Smaller than `adminArea3` or `subLocality`. For example, the postal sorting code that is used in Guernsey and many French territories, such as French Guiana; the fine-grained administrative levels in China.",
					type: "string",
					maxLength: 100,
				},
				adminArea3: {
					description:
						"A sub-locality, suburb, neighborhood, or district. Smaller than `adminArea2`. For example, in Brazil - Suburb, bairro, or neighborhood; in India - Sub-locality or district. Street name information may not always available and a sub-locality or district can reference a very small area.</li></ul>",
					type: "string",
					maxLength: 100,
				},
				adminArea2: {
					description: "A city, town, or village. Smaller than `adminArea1`.",
					type: "string",
					maxLength: 300,
				},
				adminArea1: {
					type: "string",
					description:
						"The highest level sub-division in a country, which is usually a province, state, or ISO-3166-2 subdivision; formatted for postal delivery. For example, `CA` and not `California`. For example, for UK - A county; for USs - A state; for Canada - A province; for Japan - A prefecture; for Switzerland - A kanton.",
					maxLength: 300,
				},
				postalCode: {
					type: "string",
					description:
						"The postal code, which is the zip code or equivalent. Typically required for countries that have a postal code or an equivalent. See [Postal code](https://en.wikipedia.org/wiki/Postal_code).",
					maxLength: 60,
				},
				countryCode: {
					$ref: "./country-code.json",
					description:
						"The [two-character ISO 3166-1 code](https://en.wikipedia.org/wiki/ISO_3166-1) that identifies the country or region. Note: The country code for Great Britain is `GB` and not `UK` as used in the top-level domain names for that country. Use country code `C2` for China for comparable uncontrolled price (CUP) method, bank-card, and cross-border transactions.",
				},
				addressDetails: {
					type: "object",
					title: "Address Details",
					description:
						"The non-portable additional address details that are sometimes needed for compliance, risk, or other scenarios where fine-grain address information might be needed. Not portable with common third-party and open-source address libraries and redundant with core fields. For example, `address.addressLine1` is usually a combination of `addressDetails.streetNumber` and `streetName` and `streetType`.",
					properties: {
						streetNumber: {
							description: "The street number.",
							type: "string",
							maxLength: 100,
						},
						streetName: {
							description: "The street name. Just `Drury` in `Drury Lane`.",
							type: "string",
							maxLength: 100,
						},
						streetType: {
							description:
								"The street type. For example, avenue, boulevard, road, or expressway.",
							type: "string",
							maxLength: 100,
						},
						deliveryService: {
							description:
								"The delivery service. Post office box, bag number, or post office name.",
							type: "string",
							maxLength: 100,
						},
						buildingName: {
							description:
								"A named locations that represents the premise. Usually a building name or number or collection of buildings with a common name or number. For example, <code>Craven House</code>.",
							type: "string",
							maxLength: 100,
						},
						subBuilding: {
							description:
								"The first-order entity below a named building or location that represents the sub-premise. Usually a single building within a collection of buildings with a common name. Can be a flat, story, floor, room, or apartment.",
							type: "string",
							maxLength: 100,
						},
						addressType: {
							description:
								"The type of address. Single character representation of a type. For example: 'B' for building, 'F' for organization, 'G' for general delivery, 'H' for high-rise, 'L' for large-volume organization, 'P' for Post Office box or delivery service, 'R' for rural route, 'S' for street, 'U' for unidentified address.",
							type: "string",
							maxLength: 1,
						},
						geoCoordinates: {
							description:
								"The latitude and longitude of the address. For example, `37.42242` and `-122.08585`.",
							$ref: "./geo-coordinates.json",
						},
					},
				},
			},
			required: ["countryCode"],
		},
		responseSchema: {
			$id: "https://godaddy.com/schema/common/address.v1",
			$schema: "https://json-schema.org/draft/2020-12/schema",
			type: "object",
			title: "Postal Address (Medium-Grained)",
			description:
				"An internationalized postal address. Maps to [AddressValidationMetadata](https://github.com/googlei18n/libaddressinput/wiki/AddressValidationMetadata) from Google's Address Data Service and HTML 5.1 [Autofilling form controls: the autocomplete attribute](https://www.w3.org/TR/html51/sec-forms.html#autofilling-form-controls-the-autocomplete-attribute).",
			properties: {
				addressLine1: {
					type: "string",
					description:
						"The first line of the address. For example, number and street name. For example, `3032 Bunker Hill Lane`. Required for compliance and risk checks. Must contain the full address.",
					maxLength: 300,
				},
				addressLine2: {
					type: "string",
					description:
						"The second line of the address. For example, office suite or apartment number.",
					maxLength: 300,
				},
				addressLine3: {
					type: "string",
					description:
						"The third line of the address, if needed. For example, a street complement for Brazil; direction text, such as `next to Opera House`; or a landmark reference in an Indian address.",
					maxLength: 100,
				},
				adminArea4: {
					description:
						"The neighborhood, ward, or district. Smaller than `adminArea3` or `subLocality`. For example, the postal sorting code that is used in Guernsey and many French territories, such as French Guiana; the fine-grained administrative levels in China.",
					type: "string",
					maxLength: 100,
				},
				adminArea3: {
					description:
						"A sub-locality, suburb, neighborhood, or district. Smaller than `adminArea2`. For example, in Brazil - Suburb, bairro, or neighborhood; in India - Sub-locality or district. Street name information may not always available and a sub-locality or district can reference a very small area.</li></ul>",
					type: "string",
					maxLength: 100,
				},
				adminArea2: {
					description: "A city, town, or village. Smaller than `adminArea1`.",
					type: "string",
					maxLength: 300,
				},
				adminArea1: {
					type: "string",
					description:
						"The highest level sub-division in a country, which is usually a province, state, or ISO-3166-2 subdivision; formatted for postal delivery. For example, `CA` and not `California`. For example, for UK - A county; for USs - A state; for Canada - A province; for Japan - A prefecture; for Switzerland - A kanton.",
					maxLength: 300,
				},
				postalCode: {
					type: "string",
					description:
						"The postal code, which is the zip code or equivalent. Typically required for countries that have a postal code or an equivalent. See [Postal code](https://en.wikipedia.org/wiki/Postal_code).",
					maxLength: 60,
				},
				countryCode: {
					$ref: "./country-code.json",
					description:
						"The [two-character ISO 3166-1 code](https://en.wikipedia.org/wiki/ISO_3166-1) that identifies the country or region. Note: The country code for Great Britain is `GB` and not `UK` as used in the top-level domain names for that country. Use country code `C2` for China for comparable uncontrolled price (CUP) method, bank-card, and cross-border transactions.",
				},
				addressDetails: {
					type: "object",
					title: "Address Details",
					description:
						"The non-portable additional address details that are sometimes needed for compliance, risk, or other scenarios where fine-grain address information might be needed. Not portable with common third-party and open-source address libraries and redundant with core fields. For example, `address.addressLine1` is usually a combination of `addressDetails.streetNumber` and `streetName` and `streetType`.",
					properties: {
						streetNumber: {
							description: "The street number.",
							type: "string",
							maxLength: 100,
						},
						streetName: {
							description: "The street name. Just `Drury` in `Drury Lane`.",
							type: "string",
							maxLength: 100,
						},
						streetType: {
							description:
								"The street type. For example, avenue, boulevard, road, or expressway.",
							type: "string",
							maxLength: 100,
						},
						deliveryService: {
							description:
								"The delivery service. Post office box, bag number, or post office name.",
							type: "string",
							maxLength: 100,
						},
						buildingName: {
							description:
								"A named locations that represents the premise. Usually a building name or number or collection of buildings with a common name or number. For example, <code>Craven House</code>.",
							type: "string",
							maxLength: 100,
						},
						subBuilding: {
							description:
								"The first-order entity below a named building or location that represents the sub-premise. Usually a single building within a collection of buildings with a common name. Can be a flat, story, floor, room, or apartment.",
							type: "string",
							maxLength: 100,
						},
						addressType: {
							description:
								"The type of address. Single character representation of a type. For example: 'B' for building, 'F' for organization, 'G' for general delivery, 'H' for high-rise, 'L' for large-volume organization, 'P' for Post Office box or delivery service, 'R' for rural route, 'S' for street, 'U' for unidentified address.",
							type: "string",
							maxLength: 1,
						},
						geoCoordinates: {
							description:
								"The latitude and longitude of the address. For example, `37.42242` and `-122.08585`.",
							$ref: "./geo-coordinates.json",
						},
					},
				},
			},
			required: ["countryCode"],
		},
	},
	"commerce.taxes.calculate": {
		name: "commerce.taxes.calculate",
		description: "Calculate taxes for a purchase with multiple line items",
		requestSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "commerce.taxes.calculate:request",
			type: "object",
			properties: {
				destination: {
					$ref: "#/definitions/calculationLocationInput",
				},
				lines: {
					type: "array",
					items: {
						$ref: "#/definitions/calculationLineInput",
					},
					minItems: 1,
				},
				origin: {
					$ref: "#/definitions/calculationLocationInput",
				},
			},
			required: ["lines"],
			definitions: {
				calculationLocationInput: {
					type: "object",
					properties: {
						address: {
							$ref: "#/definitions/calculationAddressInput",
						},
					},
				},
				calculationAddressInput: {
					type: "object",
					properties: {
						addressLine1: {
							type: "string",
							description:
								"The first line of the address. For example, number and street name.",
						},
						addressLine2: {
							type: "string",
							description:
								"The second line of the address. For example, office suite or apartment number.",
						},
						addressLine3: {
							type: "string",
							description: "The third line of the address, if needed.",
						},
						adminArea1: {
							type: "string",
							description:
								"The highest level sub-division in a country, which is usually a province, state, or ISO-3166-2 subdivision; formatted for postal delivery.",
						},
						adminArea2: {
							type: "string",
							description:
								"The city, town, or village. Smaller than `adminArea1`.",
						},
						adminArea3: {
							type: "string",
							description:
								"The sub-locality, suburb, neighborhood, or district. Smaller than `adminArea2`.",
						},
						adminArea4: {
							type: "string",
							description:
								"The neighborhood, ward, or district. Smaller than `adminArea3` or `subLocality`.",
						},
						countryCode: {
							type: "string",
							description:
								"The [two-character ISO 3166-1 code](https://en.wikipedia.org/wiki/ISO_3166-1) that identifies the country or region.",
						},
						postalCode: {
							type: "string",
							description:
								"The postal code, which is the zip code or equivalent.",
						},
					},
				},
				calculationLineInput: {
					type: "object",
					properties: {
						classification: {
							type: "string",
							description: "The ID or code for the item's classification.",
						},
						destination: {
							$ref: "#/definitions/calculationLocationInput",
							description:
								"The location to which the item is being sent. e.g. the customer's address. Overrides the top-level value if provided.",
						},
						id: {
							type: "string",
							description:
								"The client-defined ID for the line. This could be a UUID of the SKU, or a shipping method/package name.",
						},
						origin: {
							$ref: "#/definitions/calculationLocationInput",
							description:
								"The location from which the item is originating. Overrides the top-level value if provided.",
						},
						quantity: {
							type: "number",
							description: "The number of units being purchased.",
						},
						subtotalPrice: {
							$ref: "#/definitions/moneyInput",
							description:
								"The money amount for the total quantity being purchased.",
						},
						type: {
							type: "string",
							enum: ["FEE", "SHIPPING", "SKU"],
							description: "The type of the calculation line.",
						},
						unitPrice: {
							$ref: "#/definitions/moneyInput",
							description:
								"The money amount for a single unit of the item being purchased.",
						},
					},
					required: ["id", "subtotalPrice", "type"],
				},
				moneyInput: {
					type: "object",
					properties: {
						currencyCode: {
							type: "string",
							description: "A three-character ISO-4217 currency code.",
						},
						value: {
							type: "number",
							description:
								"The value, which might be: An integer for currencies like JPY that are not typically fractional; or, a decimal fraction for currencies like TND that are subdivided into thousandths.",
						},
					},
					required: ["currencyCode", "value"],
				},
			},
		},
		responseSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "commerce.taxes.calculate:response",
			type: "object",
			properties: {
				lines: {
					type: "array",
					items: {
						$ref: "#/definitions/calculatedLine",
					},
					minItems: 1,
				},
				taxAmounts: {
					type: "array",
					items: {
						$ref: "#/definitions/taxAmount",
					},
					description:
						"The individual tax amounts for the rates that were applied to the purchase. A combined summary of all of the rates that applied across all of the lines.",
				},
				totalTaxAmount: {
					$ref: "#/definitions/totalTaxAmount",
					description:
						"The total money amount of tax calculated for the purchase. A sum of each individual line tax amount.",
				},
			},
			required: ["lines", "taxAmounts", "totalTaxAmount"],
			definitions: {
				calculatedLine: {
					type: "object",
					properties: {
						calculationLine: {
							$ref: "#/definitions/calculationLine",
							description: "An individual line item that was calculated.",
						},
						taxAmounts: {
							type: "array",
							items: {
								$ref: "#/definitions/taxAmount",
							},
							description:
								"The individual tax amounts for the rates that were applied to the line.",
						},
						totalTaxAmount: {
							$ref: "#/definitions/totalTaxAmount",
							description:
								"The total money amount of tax calculated for the line.",
						},
					},
					required: ["calculationLine", "taxAmounts", "totalTaxAmount"],
				},
				calculationLine: {
					type: "object",
					properties: {
						id: {
							type: "string",
							description: "The client-defined ID for the line.",
						},
					},
					required: ["id"],
					description:
						"An individual line item that was used in the calculation, passed back unmodified from the request.",
				},
				taxAmount: {
					type: "object",
					properties: {
						rate: {
							$ref: "#/definitions/calculatedRate",
						},
						totalTaxAmount: {
							$ref: "#/definitions/totalTaxAmount",
						},
					},
					required: ["rate", "totalTaxAmount"],
					description: "An individual tax amount calculated.",
				},
				calculatedRate: {
					type: "object",
					properties: {
						calculationMethod: {
							type: "string",
							enum: ["ADDITIVE", "INCLUSIVE"],
							description:
								"The method that was used to apply the rate to a purchase amount.",
						},
						id: {
							type: "string",
							description: "The globally-unique ID.",
						},
						label: {
							type: "string",
							description: "The label for display.",
						},
						name: {
							type: "string",
							description: "The unique human-friendly identifier.",
						},
						value: {
							$ref: "#/definitions/calculatedRateValue",
							description: "The rate's value",
						},
					},
					required: ["calculationMethod", "value"],
					description: "The rate that was used for the calculated amount.",
				},
				calculatedRateValue: {
					oneOf: [
						{
							$ref: "#/definitions/calculatedRateAmount",
						},
						{
							$ref: "#/definitions/calculatedRatePercentage",
						},
					],
				},
				calculatedRateAmount: {
					type: "object",
					properties: {
						amount: {
							$ref: "#/definitions/money",
						},
						appliedAmount: {
							$ref: "#/definitions/money",
						},
					},
					description: "The rate value represented as a fixed money amount.",
				},
				calculatedRatePercentage: {
					type: "object",
					properties: {
						appliedPercentage: {
							type: "number",
							description: "The percentage applied by the rate, out of 100.",
						},
						percentage: {
							type: "number",
							description: "The percentage value of the rate, out of 100.",
						},
					},
					required: ["appliedPercentage", "percentage"],
					description: "The rate value represented as a percentage.",
				},
				totalTaxAmount: {
					type: "object",
					properties: {
						currencyCode: {
							type: "string",
						},
						value: {
							type: "number",
						},
					},
					required: ["currencyCode", "value"],
					description: "The total tax amount calculated.",
				},
				money: {
					type: "object",
					properties: {
						currencyCode: {
							type: "string",
							description: "A three-character ISO-4217 currency code.",
						},
						value: {
							type: "number",
							description:
								"The value, which might be: An integer for currencies like JPY that are not typically fractional; or, a decimal fraction for currencies like TND that are subdivided into thousandths.",
						},
					},
					required: ["currencyCode", "value"],
					description:
						"A money represents the monetary value and currency for a financial transaction.",
				},
			},
		},
	},
	"commerce.shipping-rates.calculate": {
		name: "commerce.shipping-rates.calculate",
		description: "Calculate shipping rates for a shipment with multiple items",
		requestSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "commerce.shipping-rates.calculate:request",
			type: "object",
			properties: {
				origin: {
					$ref: "#/definitions/address",
				},
				destination: {
					$ref: "#/definitions/address",
				},
				items: {
					type: "array",
					description: "The items to ship",
					minItems: 1,
					items: {
						$ref: "#/definitions/shipmentItem",
					},
				},
				packages: {
					type: "array",
					description: "Optionally supply the items pre-packaged for shipment.",
					items: {
						$ref: "#/definitions/dimensions",
					},
				},
				orderTotal: {
					$ref: "#/definitions/money",
					description: "The total order amount, excluding tax.",
				},
			},
			required: ["origin", "destination", "items"],
			additionalProperties: false,
			definitions: {
				address: {
					type: "object",
					properties: {
						addressLine1: {
							type: "string",
							description:
								"The first line of the address. For example, number and street name.",
						},
						addressLine2: {
							type: "string",
							description:
								"The second line of the address. For example, office suite or apartment number.",
						},
						addressLine3: {
							type: "string",
							description: "The third line of the address, if needed.",
						},
						adminArea1: {
							type: "string",
							description:
								"The highest level sub-division in a country, which is usually a province, state, or ISO-3166-2 subdivision; formatted for postal delivery.",
						},
						adminArea2: {
							type: "string",
							description:
								"The city, town, or village. Smaller than `adminArea1`.",
						},
						adminArea3: {
							type: "string",
							description:
								"The sub-locality, suburb, neighborhood, or district. Smaller than `adminArea2`.",
						},
						adminArea4: {
							type: "string",
							description:
								"The neighborhood, ward, or district. Smaller than `adminArea3` or `subLocality`.",
						},
						countryCode: {
							type: "string",
							description:
								"The [two-character ISO 3166-1 code](https://en.wikipedia.org/wiki/ISO_3166-1) that identifies the country or region.",
						},
						postalCode: {
							type: "string",
							description:
								"The postal code, which is the zip code or equivalent.",
						},
					},
					additionalProperties: false,
				},
				package: {
					type: "object",
					properties: {
						name: {
							type: "string",
						},
						maxWeight: {
							$ref: "#/definitions/weight",
							description: "Maximum weight of the package",
						},
						dimensions: {
							$ref: "#/definitions/dimensions",
							description: "Dimensions of the package",
						},
						items: {
							type: "array",
							description: "Items contained in the package",
							minItems: 1,
							items: {
								type: "object",
								properties: {
									sku: {
										type: "string",
									},
									quantity: {
										type: "integer",
										description: "Quantity of the item in the package",
									},
								},
								required: ["sku", "quantity"],
								additionalProperties: false,
							},
						},
					},
					required: ["name", "maxWeight", "dimensions", "items"],
					additionalProperties: false,
				},
				dimensions: {
					type: "object",
					properties: {
						units: {
							type: "string",
							enum: ["INCHES", "CENTIMETERS"],
							description: "Units for package dimensions",
						},
						length: {
							type: "number",
							description: "Length of the package",
						},
						width: {
							type: "number",
							description: "Width of the package",
						},
						height: {
							type: "number",
							description: "Height of the package",
						},
						weight: {
							$ref: "#/definitions/weight",
						},
					},
					required: ["units", "length", "width", "height", "weight"],
					additionalProperties: false,
				},
				weight: {
					type: "object",
					properties: {
						value: {
							type: "number",
							description: "Weight value",
						},
						units: {
							type: "string",
							enum: ["OUNCES", "POUNDS", "GRAMS", "KILOGRAMS"],
							description: "Units for weight measurement",
						},
					},
					required: ["value", "units"],
					additionalProperties: false,
				},
				shipmentItem: {
					type: "object",
					properties: {
						name: {
							type: "string",
						},
						sku: {
							type: "string",
						},
						quantity: {
							type: "integer",
						},
						price: {
							$ref: "#/definitions/money",
						},
						dimensions: {
							$ref: "#/definitions/dimensions",
						},
					},
					required: ["id"],
					additionalProperties: false,
				},
				money: {
					type: "object",
					properties: {
						currencyCode: {
							type: "string",
							description:
								"Three-letter ISO currency code (e.g., USD, EUR, GBP)",
						},
						value: {
							type: "number",
							description: "The monetary amount",
						},
					},
					required: ["currencyCode", "value"],
					additionalProperties: false,
				},
			},
		},
		responseSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "commerce.shipping-rates.calculate:response",
			type: "object",
			properties: {
				rates: {
					type: "array",
					items: {
						$ref: "#/definitions/shippingRate",
					},
					description:
						"Available shipping rates that match the request criteria",
				},
			},
			required: ["status", "rates"],
			additionalProperties: false,
			definitions: {
				shippingRate: {
					type: "object",
					properties: {
						carrierCode: {
							type: "string",
							description:
								"Code identifying the carrier (e.g., UPS, USPS, FedEx)",
						},
						serviceCode: {
							type: "string",
							description:
								"Code identifying the specific service offered by the carrier",
						},
						displayName: {
							type: "string",
							description: "Human-readable name of the shipping service",
						},
						description: {
							type: "string",
							description:
								"Additional details about the shipping service, such as estimated delivery timeframe",
						},
						minDeliveryDate: {
							type: "string",
							format: "date-time",
							description: "Earliest estimated delivery date and time",
						},
						maxDeliveryDate: {
							type: "string",
							format: "date-time",
							description: "Latest estimated delivery date and time",
						},
						cost: {
							$ref: "#/definitions/money",
							description: "The cost of the shipping service",
						},
						features: {
							type: "array",
							items: {
								type: "string",
							},
							description:
								"List of features included with this shipping rate (e.g., tracking, insurance)",
						},
					},
					required: ["carrierCode", "serviceCode", "displayName", "cost"],
					additionalProperties: false,
				},
				money: {
					type: "object",
					properties: {
						currency: {
							type: "string",
							description:
								"Three-letter ISO currency code (e.g., USD, EUR, GBP)",
						},
						amount: {
							type: "number",
							description: "The monetary amount",
						},
					},
					required: ["currency", "amount"],
					additionalProperties: false,
				},
			},
		},
	},
	"commerce.price-adjustment.apply": {
		name: "commerce.price-adjustment.apply",
		description: "Apply price adjustments (discounts and fees) to line items",
		requestSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "commerce.price-adjustment.apply:request",
			type: "object",
			properties: {
				lines: {
					type: "array",
					items: {
						$ref: "#/definitions/calculationLineInput",
					},
					minItems: 1,
				},
				discountCodes: {
					type: "array",
					items: {
						type: "string",
					},
				},
			},
			required: ["lines"],
			definitions: {
				calculationLineInput: {
					type: "object",
					properties: {
						id: {
							type: "string",
							description:
								"The client-defined ID for the line. This could be a UUID of the SKU, or a shipping method/package name. The value will be passed back unmodified in the calculation response.",
						},
						quantity: {
							type: "number",
							description: "The number of units being purchased.",
						},
						subtotalPrice: {
							$ref: "#/definitions/moneyInput",
							description:
								"The money amount for the total quantity being purchased.",
						},
						type: {
							type: "string",
							enum: ["SHIPPING", "SKU"],
							description: "The type of the calculation line.",
						},
					},
					required: ["id", "subtotalPrice", "type", "quantity"],
				},
				moneyInput: {
					type: "object",
					properties: {
						currencyCode: {
							type: "string",
							description: "A three-character ISO-4217 currency code.",
						},
						value: {
							type: "number",
							description:
								"The value, which might be: An integer for currencies like JPY that are not typically fractional; or, a decimal fraction for currencies like TND that are subdivided into thousandths.",
						},
					},
					required: ["currencyCode", "value"],
				},
			},
		},
		responseSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "commerce.price-adjustment.apply:response",
			type: "object",
			properties: {
				lines: {
					type: "array",
					items: {
						$ref: "#/definitions/calculatedLine",
					},
					minItems: 1,
				},
				adjustmentAmounts: {
					type: "array",
					items: {
						$ref: "#/definitions/calculatedAdjustment",
					},
					description:
						"The individual adjustments that were applied to the purchase. A combined summary of all of the adjustments that applied across all of the lines.",
				},
				totalDiscountAmount: {
					$ref: "#/definitions/money",
					description:
						"The total money amount of discounts calculated for the purchase.",
				},
				totalFeeAmount: {
					$ref: "#/definitions/money",
					description:
						"The total money amount of fees calculated for the purchase.",
				},
			},
			required: [
				"lines",
				"adjustments",
				"totalDiscountAmount",
				"totalDiscountAmount",
			],
			definitions: {
				calculatedLine: {
					type: "object",
					properties: {
						calculationLine: {
							$ref: "#/definitions/calculationLine",
							description: "An individual line item that was calculated.",
						},
						adjustments: {
							type: "array",
							items: {
								$ref: "#/definitions/calculatedAdjustment",
							},
							description:
								"The individual adjustments that applied to the line.",
						},
						totalDiscountAmount: {
							$ref: "#/definitions/money",
							description:
								"The total money amount of discounts calculated for the line.",
						},
						totalFeeAmount: {
							$ref: "#/definitions/money",
							description:
								"The total money amount of fees calculated for the line.",
						},
					},
					required: [
						"calculationLine",
						"adjustments",
						"totalDiscountAmount",
						"totalFeeAmount",
					],
				},
				calculationLine: {
					type: "object",
					properties: {
						id: {
							type: "string",
							description: "The client-defined ID for the line.",
						},
						type: {
							type: "string",
							description: "The line type",
						},
						unitPrice: {
							$ref: "#/definitions/money",
						},
						quantity: {
							type: "number",
							description: "The number of units being purchased",
						},
						subTotalPrice: {
							$ref: "#/definitions/money",
						},
					},
					required: ["id", "type", "subTotalPrice"],
					description:
						"An individual line item that was used in the calculation, passed back unmodified from the request.",
				},
				calculatedAdjustment: {
					type: "object",
					properties: {
						adjustment: {
							$ref: "#/definitions/adjustment",
						},
						totalAmount: {
							$ref: "#/definitions/money",
						},
					},
					required: ["adjustment", "totalAmount"],
					description: "An individual adjustment amount.",
				},
				adjustment: {
					type: "object",
					properties: {
						id: {
							type: "string",
							description: "The globally-unique ID.",
						},
						type: {
							type: "string",
							enum: ["DISCOUNT", "FEE"],
							description: "The type of adjustment.",
						},
						label: {
							type: "string",
							description: "The label for display.",
						},
						name: {
							type: "string",
							description: "The unique human-friendly identifier.",
						},
						value: {
							$ref: "#/definitions/adjustmentValue",
							description: "The adjustment's value",
						},
					},
					required: ["value", "type"],
					description:
						"The adjustment that was used for the calculated amount.",
				},
				adjustmentValue: {
					oneOf: [
						{
							$ref: "#/definitions/calculatedAdjustmentAmount",
						},
						{
							$ref: "#/definitions/calculatedAdjustmentPercentage",
						},
					],
				},
				calculatedAdjustmentAmount: {
					type: "object",
					properties: {
						amount: {
							$ref: "#/definitions/money",
						},
						appliedAmount: {
							$ref: "#/definitions/money",
						},
					},
					required: ["amount", "appliedAmount"],
					description:
						"The adjustment value represented as a fixed money amount.",
				},
				calculatedAdjustmentPercentage: {
					type: "object",
					properties: {
						appliedPercentage: {
							type: "number",
							description:
								"The percentage applied by the adjustment, out of 100.",
						},
						percentage: {
							type: "number",
							description:
								"The percentage value of the adjustment, out of 100.",
						},
					},
					required: ["appliedPercentage", "percentage"],
					description: "The adjustment value represented as a percentage.",
				},
				money: {
					type: "object",
					properties: {
						currencyCode: {
							type: "string",
							description: "A three-character ISO-4217 currency code.",
						},
						value: {
							type: "number",
							description:
								"The value, which might be: An integer for currencies like JPY that are not typically fractional; or, a decimal fraction for currencies like TND that are subdivided into thousandths.",
						},
					},
					required: ["currencyCode", "value"],
					description:
						"A money represents the monetary value and currency for a financial transaction.",
				},
			},
		},
	},
	"notifications.email.send": {
		name: "notifications.email.send",
		description: "Send an email notification",
		requestSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "notifications.email.send",
			type: "object",
			properties: {
				entityId: {
					type: "string",
					description: "UUID of the entity.",
				},
				entityType: {
					type: "string",
					description:
						"Type of UUID that the entityId refers to. Examples include: DISTRIBUTOR, STORE, CHANNEL.",
				},
				envelope: {
					$ref: "#/definitions/emailEnvelope",
				},
				context: {
					$ref: "#/definitions/context",
				},
			},
			required: ["entityId", "entityType", "envelope", "context"],
			definitions: {
				context: {
					type: "object",
					description: "Context for the email",
					readonly: true,
					properties: {
						owner: {
							type: "string",
							description:
								"A distinct identifier of which system or service creating the email to send.",
							example: "urn:com.godaddy:commerce.order",
						},
						storeId: {
							type: "string",
							description: "ID of the store that the email belongs to",
							example: "fd3d9291-dc9a-445d-aa68-d2ff1a77a83c",
						},
						topic: {
							type: "string",
							description:
								"A distinct identifier of what the email represents.",
							example: "delivery-confirmed",
						},
						objectType: {
							type: "string",
							description: "The object type the email is about.",
							example: "order",
						},
						objectId: {
							type: "string",
							description: "The object ID the email is about.",
							example: "Order_30CkE0hWAITMFEYEFZKEDBbtiQc",
						},
						references: {
							type: "array",
							description: "A set of references associated with the email.",
							items: {
								$ref: "#/definitions/Reference",
							},
						},
					},
					required: ["owner", "topic", "objectType", "objectId", "storeId"],
				},
				emailEnvelope: {
					type: "object",
					properties: {
						to: {
							type: "array",
							items: {
								$ref: "#/definitions/addressOrString",
							},
						},
						subject: {
							type: "string",
							description: "Subject of the email.",
						},
						body: {
							type: "string",
							description: "Body of the email.",
						},
						isHtml: {
							type: "boolean",
							description: "Whether the email contains HTML.",
						},
						sentFrom: {
							$ref: "#/definitions/addressOrString",
						},
					},
					required: ["to", "subject", "body"],
				},
				addressOrString: {
					oneOf: [
						{
							type: "string",
							description:
								"An email address on its own or email header e.g. Jane<jane@example.com>",
						},
						{
							$ref: "#/definitions/address",
						},
					],
				},
				address: {
					type: "object",
					properties: {
						email: {
							type: "string",
							description: "Email address.",
						},
						name: {
							type: "string",
							description: "Name of the person who owns the email address.",
						},
					},
					required: ["email"],
				},
				Reference: {
					description:
						"A reference to the resource in an external service. This can be useful for integrating the resource to any external service.",
					type: "object",
					required: ["id", "value", "origin"],
					properties: {
						type: {
							type: "string",
							description: "The unique UUID of the reference.",
							example: "fd3d9291-dc9a-445d-aa68-d2ff1a77a83c",
							readonly: true,
						},
						value: {
							type: "string",
							description:
								"The identifier of the resource in an external service.",
							example: "B0CX23PQVP",
						},
						origin: {
							type: "string",
							description: "The origin or location of the reference.",
							example: "OLS",
						},
					},
				},
			},
		},
		responseSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "notifications.email.send:response",
			type: "object",
			description: "200 OK response with empty body",
			properties: {},
			additionalProperties: false,
		},
	},
	"commerce.payment.process": {
		name: "commerce.payment.process",
		description: "Process a payment by creating a new SALE transaction",
		requestSchema: {
			$id: "https://godaddy.com/schemas/commerce/create-sale-transaction.v2",
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "Create Sale Transaction",
			description:
				"Schema for creating a new SALE transaction to process a payment",
			type: "object",
			properties: {
				type: {
					const: "SALE",
					description: "Transaction type - must be SALE for payment processing",
				},
				transactionRefNum: {
					$ref: "#/$defs/RefNum",
					description: "Reference number for the transaction",
				},
				fundingSource: {
					$ref: "#/$defs/FundingSource",
					description: "Payment method to be used for the transaction",
				},
				amount: {
					$ref: "#/$defs/DetailedAmount",
					description: "Transaction amount to be processed",
				},
				context: {
					$ref: "#/$defs/TransactionContext",
					description:
						"Additional context about where the transaction was initiated",
				},
				clientContext: {
					$ref: "#/$defs/Dictionary",
					description:
						"Non-sensitive context data that can be stored and fetched by client",
				},
				processingInstructions: {
					$ref: "#/$defs/TransactionProcessingInstruction",
					description: "Instructions for processing the transaction",
				},
				notes: {
					type: "string",
					maxLength: 512,
					description: "Optional notes about the transaction",
				},
				references: {
					type: "array",
					items: {
						$ref: "#/$defs/TransactionReference",
					},
					description: "References to orders, invoices, or other documents",
				},
			},
			required: ["type", "amount", "fundingSource"],
			additionalProperties: false,
			$defs: {
				RefNum: {
					title: "ReferenceNumber",
					description: "Reference number (UUID format)",
					type: "string",
					format: "uuid",
				},
				Dictionary: {
					title: "Dictionary",
					description: "Key value pairs of strings",
					type: "object",
					minProperties: 0,
					maxProperties: 40,
					additionalProperties: {
						type: "string",
						maxLength: 256,
					},
				},
				Currency: {
					title: "Currency",
					description: "ISO 4217 Currency codes",
					type: "string",
					example: "USD",
					minLength: 3,
					maxLength: 3,
				},
				Uuid: {
					title: "Uuid",
					description: "Unique identifier (UUID format)",
					type: "string",
					format: "uuid",
				},
				DetailedAmount: {
					title: "Detailed Amount",
					type: "object",
					description: "Itemized amount details for payment processing",
					properties: {
						amountType: {
							const: "DETAILED",
						},
						total: {
							type: "integer",
							format: "int64",
							minimum: 1,
							description: "Total amount in smallest currency unit",
						},
						tip: {
							$ref: "#/$defs/TipAmount",
							description: "Tip amount details",
						},
						cashback: {
							type: "integer",
							format: "int64",
							minimum: 0,
							description: "Cashback amount requested",
						},
						subTotal: {
							type: "integer",
							format: "int64",
							minimum: 0,
							description: "Subtotal before tips and fees",
						},
						currency: {
							$ref: "#/$defs/Currency",
							description: "Amount currency",
						},
						fees: {
							type: "array",
							items: {
								$ref: "#/$defs/Fee",
							},
							description: "Additional fees applied to the transaction",
						},
					},
					required: ["amountType", "total", "currency"],
					additionalProperties: false,
				},
				TipAmount: {
					title: "Tip Amount",
					type: "object",
					description: "Tip amount details",
					properties: {
						amount: {
							type: "integer",
							format: "int64",
							minimum: 0,
							description: "Tip amount in smallest currency unit",
						},
						customerOptedNoTip: {
							type: "boolean",
							default: false,
							description: "Indicates if customer explicitly opted for no tip",
						},
					},
					additionalProperties: false,
				},
				Fee: {
					title: "Fee",
					type: "object",
					description: "Fee details",
					properties: {
						type: {
							type: "string",
							description: "Type of fee",
						},
						amount: {
							type: "integer",
							format: "int64",
							minimum: 0,
							description: "Fee amount in smallest currency unit",
						},
						description: {
							type: "string",
							description: "Description of the fee",
						},
					},
					required: ["type", "amount"],
					additionalProperties: false,
				},
				TransactionContext: {
					title: "Transaction Context",
					type: "object",
					description:
						"Context details about where the transaction was initiated",
					properties: {
						channelId: {
							$ref: "#/$defs/Uuid",
							description: "Channel identifier where transaction was initiated",
						},
						merchantInitiatedTransaction: {
							type: "boolean",
							default: false,
							description:
								"True if initiated by merchant on behalf of customer",
						},
					},
					additionalProperties: false,
				},
				FundingSource: {
					title: "Funding Source",
					type: "object",
					description: "Payment method for the transaction",
					oneOf: [
						{
							$ref: "#/$defs/CustomFundingSource",
						},
					],
					discriminator: {
						propertyName: "sourceType",
					},
				},
				CustomFundingSource: {
					title: "Custom Funding Source",
					type: "object",
					description: "Custom payment method processed by a specific provider",
					properties: {
						sourceType: {
							const: "CUSTOM",
						},
						provider: {
							type: "string",
							maxLength: 64,
							description: "Provider that can handle the funding source",
						},
						processor: {
							type: "string",
							maxLength: 64,
							description: "Processor to be used for the funding source",
						},
						customFundingType: {
							type: "string",
							maxLength: 64,
							description: "Type of the custom funding source",
						},
						name: {
							type: "string",
							maxLength: 64,
							description: "Display name of the funding source",
						},
						description: {
							type: "string",
							maxLength: 512,
							description: "Description about the funding source",
						},
						paymentReference: {
							type: "string",
							description:
								"The reference to the payment token for the funding source",
						},
					},
					required: [
						"sourceType",
						"customFundingType",
						"provider",
						"paymentReference",
					],
					additionalProperties: false,
				},
				TransactionProcessingInstruction: {
					title: "Transaction Processing Instruction",
					description: "Instructions for processing the transaction",
					type: "object",
					properties: {
						partialAuthEnabled: {
							type: "boolean",
							default: true,
							description: "Allow partial authorizations for the sale",
						},
						statementDescriptorSuffix: {
							type: "string",
							maxLength: 22,
							description: "Additional text to appear on customer's statement",
						},
						storeAndForward: {
							type: "boolean",
							default: false,
							description: "Process as offline transaction (store and forward)",
						},
					},
					additionalProperties: false,
				},
				TransactionReference: {
					title: "Transaction Reference",
					type: "object",
					description: "Reference to an order, invoice, or other document",
					properties: {
						value: {
							type: "string",
							maxLength: 128,
							description: "Reference value (e.g., order number, invoice ID)",
						},
						type: {
							type: "string",
							maxLength: 32,
							description:
								"Type of reference (e.g., 'ORDER', 'INVOICE', 'SUBSCRIPTION')",
						},
						additionalLabel: {
							type: "string",
							maxLength: 32,
							description: "Optional additional label for the reference",
						},
					},
					required: ["value", "type"],
					additionalProperties: false,
				},
			},
		},
		responseSchema: {
			$id: "https://godaddy.com/schemas/commerce/transaction-complete.v2",
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "Complete Transaction Schema",
			description: "Full Transaction type with all referenced types expanded",
			type: "object",
			properties: {
				type: {
					const: "SALE",
				},
				transactionId: {
					type: "string",
					pattern:
						"([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})|(urn:[a-z]{1,3}:[a-z]{1,3}:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})",
					description: "Server generated identifier for this transaction",
				},
				transactionRefNum: {
					type: "string",
					format: "uuid",
					description: "Reference number (UUID format)",
				},
				storeId: {
					type: "string",
					format: "uuid",
					description: "Store identifier for this transaction",
				},
				fundingSource: {
					type: "object",
					properties: {
						sourceType: {
							const: "CUSTOM",
						},
						customFundingType: {
							type: "string",
							description: "Type of the custom funding source",
						},
						provider: {
							type: "string",
							description: "Provider that can handle the custom funding source",
						},
						processor: {
							type: "string",
							description: "Processor used for the custom funding source",
						},
						name: {
							type: "string",
							description: "Display name of the funding source",
						},
						description: {
							type: "string",
							description: "Display description for the funding source",
						},
						paymentReference: {
							type: "string",
							description:
								"The ID to the processed payment from the funding source",
						},
					},
					required: [
						"sourceType",
						"customFundingType",
						"provider",
						"paymentReference",
					],
				},
				status: {
					type: "string",
					enum: ["INITIATED", "PENDING", "FAILED", "COMPLETED", "VOIDED"],
					description: "Transaction status",
				},
				amount: {
					type: "object",
					properties: {
						amountType: {
							const: "DETAILED",
						},
						total: {
							type: "integer",
							format: "int64",
							description: "Total amount",
						},
						currency: {
							type: "string",
							minLength: 3,
							maxLength: 3,
							description: "Amount currency",
						},
					},
					required: ["amountType", "total", "currency"],
				},
				createdAt: {
					type: "string",
					format: "date-time",
					description: "Created time in RFC 3339 format",
					example: "2022-07-21T17:32:28Z",
				},
			},
			required: ["type", "amount", "createdAt"],
		},
	},
	"commerce.payment.refund": {
		name: "commerce.payment.refund",
		description: "Refund a payment transaction",
		requestSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "RefundPaymentRequest",
			type: "object",
			properties: {
				transactionId: {
					type: "string",
					description: "ID of the transaction to refund",
				},
				amount: {
					type: "object",
					properties: {
						currency: {
							type: "string",
							description: "Three-letter ISO currency code",
						},
						value: {
							type: "number",
							description: "Amount to refund",
						},
					},
					required: ["currency", "value"],
				},
				reason: {
					type: "string",
					description: "Reason for the refund",
				},
				metadata: {
					type: "object",
					additionalProperties: {
						type: "string",
					},
					description: "Additional metadata for the refund",
				},
			},
			required: ["transactionId", "amount"],
		},
		responseSchema: {
			$schema: "https://json-schema.org/draft/2020-12/schema",
			title: "RefundPaymentResponse",
			type: "object",
			properties: {
				refundId: {
					type: "string",
					description: "Unique identifier for the refund",
				},
				status: {
					type: "string",
					description: "Status of the refund",
				},
				createdAt: {
					type: "string",
					format: "date-time",
					description: "When the refund was created",
				},
				amount: {
					type: "object",
					properties: {
						currency: {
							type: "string",
							description: "Three-letter ISO currency code",
						},
						value: {
							type: "number",
							description: "Amount refunded",
						},
					},
					required: ["currency", "value"],
				},
				reason: {
					type: "string",
					description: "Reason for the refund",
				},
				metadata: {
					type: "object",
					additionalProperties: {
						type: "string",
					},
					description: "Additional metadata for the refund",
				},
			},
			required: ["refundId", "status", "createdAt", "amount"],
		},
	},
	// Other actions will be added here
};

export function createActionsCommand(): Command {
	const actions = new Command("actions").description(
		"Manage application actions",
	);

	// actions list
	actions
		.command("list")
		.description(
			"List all available actions that an application developer can hook into",
		)
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (options) => {
			const availableActions = AVAILABLE_ACTIONS;

			if (options.output === "json") {
				console.log(JSON.stringify({ actions: availableActions }, null, 2));
			} else {
				if (availableActions.length === 0) {
					console.log("No actions found");
					return;
				}

				console.log(`Available actions (${availableActions.length}):`);
				for (const action of availableActions) {
					console.log(`  ${action}`);
				}
			}
			process.exit(0);
		});

	// actions describe <action-name>
	actions
		.command("describe")
		.description("Show detailed interface information for a specific action")
		.argument("<action>", "Action name")
		.option("-o, --output <format>", "Output format (json|text)", "text")
		.action(async (actionName, options) => {
			if (!AVAILABLE_ACTIONS.includes(actionName)) {
				console.error(`Action '${actionName}' not found`);
				console.log(`Available actions: ${AVAILABLE_ACTIONS.join(", ")}`);
				process.exit(1);
			}

			const actionInterface = ACTION_INTERFACES[actionName];

			if (!actionInterface) {
				console.error(
					`Interface definition not available for action '${actionName}'`,
				);
				process.exit(1);
			}

			if (options.output === "json") {
				console.log(JSON.stringify(actionInterface, null, 2));
			} else {
				console.log(`Action: ${actionInterface.name}`);
				console.log(`Description: ${actionInterface.description}`);
				console.log("");
				console.log("Request Schema:");
				console.log(JSON.stringify(actionInterface.requestSchema, null, 2));
				console.log("");
				console.log("Response Schema:");
				console.log(JSON.stringify(actionInterface.responseSchema, null, 2));
			}
			process.exit(0);
		});

	return actions;
}
